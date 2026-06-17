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

use reify_ir::value::SampledField;

use crate::grid_validation::{GridValidationError, validate_regular3d};

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
///
/// `#[non_exhaustive]` lets future variants be added without breaking
/// external exhaustive-match consumers.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum MedialError {
    /// A structural validation error produced by the shared
    /// [`crate::grid_validation::validate_regular3d`] check. Covers
    /// unsupported grid kind, axis-vector length mismatch, and empty
    /// axis-grid — see [`GridValidationError`] variants for details.
    GridValidation(GridValidationError),
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
    /// The product `nx * ny * nz` (where `nx/ny/nz = axis_grids[i].len()`)
    /// overflows `usize` when computing the expected flat-data length.
    ///
    /// Defends the `DataLengthMismatch` check (and downstream `data[i*nj*nk
    /// + j*nk + k]` indexing) from a wrapped-product false-positive: without
    /// this variant, an overflow would fall back to `unwrap_or(usize::MAX)`
    /// and surface as a spurious `DataLengthMismatch { expected: usize::MAX,
    ///   found: actual }` — a confusing sentinel that lies about which condition
    /// fired. The dedicated variant carries the actual extent values so
    /// callers can report precisely why the product cannot fit in `usize`.
    AxisExtentsOverflow {
        /// Length of `axis_grids[0]`.
        nx: usize,
        /// Length of `axis_grids[1]`.
        ny: usize,
        /// Length of `axis_grids[2]`.
        nz: usize,
    },
}

impl From<GridValidationError> for MedialError {
    fn from(e: GridValidationError) -> Self {
        MedialError::GridValidation(e)
    }
}

impl std::fmt::Display for MedialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MedialError::GridValidation(inner) => write!(f, "compute_medial_mask: {inner}"),
            MedialError::InvalidAxisGeometry {
                axis,
                spacing,
                bounds_min,
                bounds_max,
            } => {
                let sp_bad = !(spacing.is_finite() && *spacing > 0.0);
                let bn_bad =
                    !bounds_min.is_finite() || !bounds_max.is_finite() || bounds_min > bounds_max;
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
                        "Regular3D SampledField axis {axis}: no violation detected \
                         (spacing={spacing}, bounds_min={bounds_min}, \
                         bounds_max={bounds_max}) \
                         — variant was constructed outside the validator"
                    ),
                }
            }
            MedialError::DataLengthMismatch { expected, found } => write!(
                f,
                "Regular3D SampledField data length mismatch: \
                 expected {expected} values (nx*ny*nz) but found {found} \
                 (caller-side construction error: flat data does not match axis grid extents)"
            ),
            MedialError::AxisExtentsOverflow { nx, ny, nz } => write!(
                f,
                "Regular3D SampledField axis extents nx={nx}, ny={ny}, nz={nz} \
                 overflow usize when computing the flat data length nx*ny*nz \
                 (caller-side construction error: axis grid lengths are individually valid \
                 but their product cannot fit in usize)"
            ),
        }
    }
}

impl std::error::Error for MedialError {}

/// Measured minimum wall thickness with honest-floor semantics.
///
/// PRD §3b / task δ (4424). The min-wall measurement is the MIN of d⁺+d⁻ over
/// all medial voxels — a conservative-lower-bound bias. `BelowResolution`
/// self-describes when the raw min-wall is below the `2·h` floor (G6
/// honest-floor: never silently promoted to `Measured`). The consumer ζ=4426
/// maps `BelowResolution` and `NoMeasurement` to `Indeterminate` rather than
/// using a sub-resolution number.
#[derive(Debug, Clone, PartialEq)]
pub enum MinWallThickness {
    /// Measured minimum wall thickness (same units as the SDF grid spacing).
    /// Guaranteed ≥ `2·h` (the resolution floor); conservative-lower-bound
    /// via the min-reduction over all medial voxels.
    Measured(f64),
    /// The raw measured min-wall (`raw`) is below the `2·h` resolution floor
    /// (`floor = 2·h`).  The raw value is still carried so the caller can
    /// decide how to report it; it must NOT be silently treated as a reliable
    /// physical thickness.
    BelowResolution {
        /// Raw min-wall sum d⁺+d⁻ from the walk, < `floor`.
        raw: f64,
        /// Resolution floor = 2 × voxel spacing `h`.
        floor: f64,
    },
    /// No measurable medial voxels — either the mask is empty, or every
    /// medial voxel failed the gradient/walk guards (degenerate gradient,
    /// out-of-bounds walk, or non-finite d⁺+d⁻ sum).  The input SDF does
    /// not yield a reliable wall-thickness estimate at the given resolution.
    NoMeasurement,
}

/// Compute the minimum wall thickness of a thin solid from its narrow-band SDF.
///
/// Runs `compute_medial_mask(sdf, &MedialOptions::default())` to identify
/// medial voxels, re-walks each via `bidirectional_distances`, and returns the
/// MIN of `d⁺+d⁻` — the conservative-lower-bound min-wall scalar (bias-low:
/// the min-reduction can only underestimate, never overestimate).
///
/// The explicit `h` parameter (voxel spacing, typically `min(sdf.spacing)`) is
/// the resolution floor used for the `BelowResolution` branch.  The eval
/// binding (`Engine::measure_min_wall`, task δ) derives `h` from the realized
/// grid's own spacing, decoupling this function from any external voxelisation
/// default.
///
/// # Returns
///
/// - `Ok(Measured(t))` — `t ≥ 2·h`; conservative lower bound.
/// - `Ok(BelowResolution { raw, floor })` — `raw < 2·h = floor`.
/// - `Ok(NoMeasurement)` — empty mask or every medial voxel failed its
///   gradient/walk guard; see `MinWallThickness::NoMeasurement`.
/// - `Err(MedialError)` — structurally invalid SDF (same conditions as
///   `compute_medial_mask`).
///
/// # Performance
///
/// `min_wall_thickness` re-walks the medial voxels via `bidirectional_distances`
/// rather than caching per-voxel distances from `compute_medial_mask` (which
/// computes them internally but discards them).  The mask is O(100 voxels) for
/// PRD-typical 3–10-voxel walls, so the extra pass is negligible for
/// single-part evaluations.  This layering avoids restructuring
/// `compute_medial_mask`'s parallel chunk-merge return type — a heavily-tested
/// function that should stay untouched.  A cached variant can be introduced in
/// a follow-up if profiling shows hotness.
pub fn min_wall_thickness(
    sdf: &SampledField,
    h: f64,
) -> Result<MinWallThickness, MedialError> {
    let mask = compute_medial_mask(sdf, &MedialOptions::default())?;
    if mask.voxels.is_empty() {
        return Ok(MinWallThickness::NoMeasurement);
    }

    // Walk parameters via shared walk_params() helper — guaranteed to stay
    // in sync with compute_medial_mask (eliminates the hand-copy drift risk).
    let options = MedialOptions::default();
    let min_spacing = sdf.spacing[0].min(sdf.spacing[1]).min(sdf.spacing[2]);
    let (max_steps, walk_step, _max_walk_dist) = walk_params(min_spacing, &options);

    let nx = sdf.axis_grids[0].len();
    let ny = sdf.axis_grids[1].len();
    let nz = sdf.axis_grids[2].len();

    let mut min_sum = f64::INFINITY;

    for &[vi, vj, vk] in &mask.voxels {
        // Bounds-guard: medial mask indices must be within the grid.
        let idx = [vi as usize, vj as usize, vk as usize];
        if idx[0] >= nx || idx[1] >= ny || idx[2] >= nz {
            continue;
        }

        // World coordinate and normalised gradient for this medial voxel.
        let world = world_at_index(sdf, idx);
        let grad_raw = gradient_at_index(sdf, idx);
        let Some(g) = normalize3(grad_raw) else {
            continue; // degenerate gradient — skip
        };

        // Bidirectional walk: d⁺ + d⁻ for this voxel.
        let Some((d_plus, d_minus, _, _)) =
            bidirectional_distances(sdf, world, g, max_steps, walk_step)
        else {
            continue; // walk failed (off-grid) — skip
        };

        let sum = d_plus + d_minus;
        if sum.is_finite() {
            min_sum = min_sum.min(sum);
        }
    }

    if !min_sum.is_finite() {
        return Ok(MinWallThickness::NoMeasurement);
    }

    // G6 honest-floor (PRD §3b / task δ step-4): split on raw vs. resolution
    // floor. The threshold is `2·h` (two voxel-widths) — the smallest
    // distance that can be reliably resolved by the bidirectional walk at
    // voxel size h. The decision is on the RAW sum (not a floored copy)
    // so the 2h threshold semantics stay exact.
    let floor = 2.0 * h;
    if min_sum < floor {
        Ok(MinWallThickness::BelowResolution { raw: min_sum, floor })
    } else {
        Ok(MinWallThickness::Measured(min_sum))
    }
}

// ── ε=4425: MinFeatureSize + min_feature_size_measure ────────────────────────

/// Outcome of `min_feature_size_measure` — the min-feature scalar measured from
/// the medial-axis voxels of an SDF via `2×min|φ|` over all ridge voxels.
///
/// Mirrors [`MinWallThickness`] (task δ=4424) exactly; only the per-voxel
/// reduction differs (`2|φ|` via `sample_at_index` instead of `d⁺+d⁻` via
/// the bidirectional walk).  The conservative-lower-bound bias is the PRD's
/// required property: on a voxel grid the medial mask tags the nearest
/// off-mid-plane voxels, so `2|φ|` reads slightly LOW — never an over-read.
/// `BelowResolution` self-describes when the raw value is below the `2·h`
/// floor (G6 honest-floor: never silently promoted to `Measured`).  Consumer
/// ζ=4426 maps `BelowResolution` and `NoMeasurement` to `Indeterminate`.
#[derive(Debug, Clone, PartialEq)]
pub enum MinFeatureSize {
    /// Measured minimum feature size (same units as the SDF grid spacing).
    /// Guaranteed ≥ `2·h` (the resolution floor); conservative-lower-bound
    /// via the min-reduction over all ridge voxels.
    Measured(f64),
    /// The raw measured min-feature (`raw`) is below the `2·h` resolution floor
    /// (`floor = 2·h`).  The raw value is still carried so the caller can
    /// decide how to report it; it must NOT be silently treated as a reliable
    /// physical feature size.
    BelowResolution {
        /// Raw min-feature `2×|φ|`, < `floor`.
        raw: f64,
        /// Resolution floor = 2 × voxel spacing `h`.
        floor: f64,
    },
    /// No measurable ridge voxels — either the medial mask is empty, or every
    /// ridge voxel has a non-finite `|φ|`.  The input SDF does not yield a
    /// reliable feature-size estimate at the given resolution.
    NoMeasurement,
}

/// Compute the minimum feature size of a thin solid from its narrow-band SDF.
///
/// Runs `compute_medial_mask(sdf, &MedialOptions::default())` to identify
/// ridge (medial-axis) voxels, reads `|φ|` at each via `sample_at_index`
/// (raw grid value), and returns `2 × min|φ|` — the conservative-lower-bound
/// min-feature scalar (bias-low: at a true medial point `|φ|` = half the local
/// material thickness; on the nearest off-mid-plane voxels the grid tags,
/// `2|φ|` underestimates by up to ~h).
///
/// The explicit `h` parameter (voxel spacing, typically `min(sdf.spacing)`) is
/// the resolution floor used for the `BelowResolution` branch.  The eval
/// binding (`Engine::measure_min_feature`, task ε) derives `h` from the
/// realized grid's own spacing.
///
/// # Returns
///
/// - `Ok(Measured(t))` — `t ≥ 2·h`; conservative lower bound.
/// - `Ok(BelowResolution { raw, floor })` — `raw < 2·h = floor`.
/// - `Ok(NoMeasurement)` — empty mask or every ridge voxel had non-finite `|φ|`.
/// - `Err(MedialError)` — structurally invalid SDF (same conditions as
///   `compute_medial_mask`).
pub fn min_feature_size_measure(
    sdf: &SampledField,
    h: f64,
) -> Result<MinFeatureSize, MedialError> {
    let mask = compute_medial_mask(sdf, &MedialOptions::default())?;
    if mask.voxels.is_empty() {
        return Ok(MinFeatureSize::NoMeasurement);
    }

    let nx = sdf.axis_grids[0].len();
    let ny = sdf.axis_grids[1].len();
    let nz = sdf.axis_grids[2].len();

    let mut min_abs = f64::INFINITY;

    for &[vi, vj, vk] in &mask.voxels {
        // Bounds-guard: medial mask indices must be within the grid.
        let idx = [vi as usize, vj as usize, vk as usize];
        if idx[0] >= nx || idx[1] >= ny || idx[2] >= nz {
            continue;
        }

        let phi = sample_at_index(sdf, idx);
        if phi.is_finite() {
            min_abs = min_abs.min(phi.abs());
        }
    }

    if !min_abs.is_finite() {
        return Ok(MinFeatureSize::NoMeasurement);
    }

    // NOTE: honest-floor branch added in step-4 (ε=4425).
    // For now always return Measured (step-2 GREEN, no floor).
    Ok(MinFeatureSize::Measured(2.0 * min_abs))
}

// ── end ε=4425 ────────────────────────────────────────────────────────────────

/// Compute the bidirectional walk parameters from the minimum grid spacing.
///
/// Shared by `min_wall_thickness` and `compute_medial_mask` so that the two
/// sites cannot silently diverge if the step factor (currently 0.25) or the
/// minimum-steps floor (currently 2) ever changes.
///
/// Returns `(max_steps, walk_step, max_walk_dist)`.
fn walk_params(min_spacing: f64, options: &MedialOptions) -> (usize, f64, f64) {
    // Absolute truncation distance for the bidirectional walk.
    let max_walk_dist = options.max_thickness_voxels * min_spacing;
    // Step size: 1/4 of the smallest voxel spacing keeps sub-voxel
    // zero-crossing refinement accurate while bounding per-voxel work to
    // ≈ 4 × max_thickness_voxels samples.
    let walk_step = 0.25 * min_spacing;
    let max_steps = ((max_walk_dist / walk_step).ceil() as usize).max(2);
    (max_steps, walk_step, max_walk_dist)
}

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
    // (1) Structural Regular3D validation: kind, axis-vector lengths, non-empty
    // axis grids. The `?` converts GridValidationError → MedialError via the
    // From impl above, preserving the existing variant names and PartialEq
    // contract for all callers.
    validate_regular3d(sdf)?;

    // (2) Geometry validity: spacing must be finite and positive, and
    // bounds must not be inverted. Safe here because validate_regular3d
    // confirmed all axis vectors have length 3, so sdf.spacing[axis] /
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

    let spacing = [sdf.spacing[0], sdf.spacing[1], sdf.spacing[2]];
    let origin = [sdf.bounds_min[0], sdf.bounds_min[1], sdf.bounds_min[2]];

    let nx = sdf.axis_grids[0].len();
    let ny = sdf.axis_grids[1].len();
    let nz = sdf.axis_grids[2].len();

    // (3) Validate the flat data vector covers exactly nx*ny*nz voxels.
    // Delegates to `validate_flat_data_length` which handles both overflow
    // (returns AxisExtentsOverflow) and size mismatch (returns
    // DataLengthMismatch), replacing the old unwrap_or(usize::MAX) sentinel.
    validate_flat_data_length(nx, ny, nz, sdf.data.len())?;

    // Narrow band threshold uses the smallest axis spacing so that
    // anisotropic grids still cover the full thickness band.
    let min_spacing = spacing[0].min(spacing[1]).min(spacing[2]);
    let band_width = options.narrow_band_half_width_voxels * min_spacing;

    // Walk truncation-distance + step size via shared walk_params() so that
    // min_wall_thickness's re-walk always uses the same formula.
    let (max_steps, walk_step, max_walk_dist) = walk_params(min_spacing, options);

    // Pre-compute the per-voxel gradient once before the main loop.
    // Avoids repeating the 6-sample central-difference stencil inside
    // the hot path; the lookup is O(1) via i*ny*nz + j*nz + k.
    let gradient_grid = precompute_gradient_grid(sdf, band_width);

    // Parallel outer (i-axis) loop.
    //
    // Determinism contract (mirrors crates/reify-solver-elastic/src/assembly/global.rs § "# Determinism contract"):
    // (a) `i_indices.chunks(chunk_size)` partitions indices in stable
    //     ascending order (chunks() is a stable slice partition);
    // (b) threads spawn in chunk-iteration order, so handle slot `t`
    //     corresponds to chunk `t` regardless of OS thread assignment;
    // (c) `for h in handles` joins in spawn order and appends in that
    //     order — preserving spawn order in the merged Vec;
    // (d) `sort_unstable()` normalises the merged Vec to strict lex
    //     order, independent of future chunk-distribution changes.
    //     Duplicates cannot appear: each (i,j,k) is visited by exactly
    //     one thread's chunk.
    // Per Task-2544: panics in workers are forwarded via `resume_unwind`
    // so the original payload reaches the caller intact.
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(nx.max(1));
    let chunk_size = nx.div_ceil(threads).max(1);
    let i_indices: Vec<usize> = (0..nx).collect();
    // Extract Copy scalars from `options` so closures capture by value
    // (required for `Send` — closures cannot hold a `&MedialOptions`
    // borrow across the scope boundary when `options` is not `Sync`-
    // verified by the caller; extracting Copy fields is safer).
    let dist_tol = options.distance_tolerance;
    let antipar_thr = options.normal_antiparallel_threshold;
    // Shared immutable borrow of gradient_grid across all threads.
    // `&[[f64;3]]` is `Copy + Send` (since `[f64;3]: Sync`), so each
    // `move` closure copies the fat-pointer rather than moving the Vec.
    let gradient_grid_ref: &[[f64; 3]] = &gradient_grid;

    let mut voxels: Vec<[i32; 3]> = std::thread::scope(|s| {
        let mut handles = Vec::with_capacity(threads);
        for chunk in i_indices.chunks(chunk_size) {
            handles.push(s.spawn(move || {
                // Pre-size to ~1/32 of the chunk's voxel count; the narrow-band
                // filter rejects ≥95% of voxels in typical slab/sphere fixtures,
                // so this starting capacity avoids most reallocations without
                // over-allocating.
                let mut local: Vec<[i32; 3]> = Vec::with_capacity(chunk.len() * ny * nz / 32);
                for &i in chunk {
                    for j in 0..ny {
                        for k in 0..nz {
                            let phi = sample_at_index(sdf, [i, j, k]);

                            // (a) narrow-band filter
                            if phi.abs() > band_width {
                                continue;
                            }

                            // (b) gradient at the voxel; reject degenerate.
                            //
                            // Invariant: `precompute_gradient_grid` only computes
                            // slots where |φ| ≤ band_width; out-of-band slots are
                            // the [0.0; 3] sentinel and are structurally unreachable
                            // here because the `continue` above gates on the SAME
                            // `phi` computed from the SAME `sample_at_index` call.
                            // The debug_assert enforces this coupling so that a
                            // future refactor (e.g. different threshold in one side)
                            // fails loudly in debug builds rather than silently
                            // emitting a degenerate-gradient skip.
                            debug_assert!(
                                phi.abs() <= band_width,
                                "gradient cache indexed for out-of-band voxel: \
                                 |phi|={phi} > band_width={band_width}"
                            );
                            let grad = gradient_grid_ref[i * ny * nz + j * nz + k];
                            let gnorm =
                                (grad[0] * grad[0] + grad[1] * grad[1] + grad[2] * grad[2]).sqrt();
                            if gnorm < GRADIENT_EPSILON {
                                continue;
                            }
                            let g = [grad[0] / gnorm, grad[1] / gnorm, grad[2] / gnorm];

                            // (c) bidirectional ray walk from the voxel's
                            // world coordinate in ±g, with sub-voxel
                            // zero-crossing refinement.
                            let world = world_at_index(sdf, [i, j, k]);
                            let Some((d_plus, d_minus, hit_plus, hit_minus)) =
                                bidirectional_distances(sdf, world, g, max_steps, walk_step)
                            else {
                                continue;
                            };

                            // (d) gradient at each surface hit; reject if
                            // either is degenerate (we cannot make a
                            // distinctness call without two well-defined
                            // normals).
                            let gp_raw = gradient_at_world(sdf, hit_plus);
                            let gm_raw = gradient_at_world(sdf, hit_minus);
                            let Some(gp) = normalize3(gp_raw) else {
                                continue;
                            };
                            let Some(gm) = normalize3(gm_raw) else {
                                continue;
                            };

                            // (e) gradient-discontinuity test: opposing-face
                            // hits have antiparallel normals (dot near -1).
                            if !surface_patches_distinct(gp, gm, antipar_thr) {
                                continue;
                            }

                            // (f) bidirectional-distance equality.
                            let dmax = d_plus.max(d_minus);
                            if dmax <= 0.0 {
                                continue;
                            }
                            // Defends against a long-walk-on-one-side /
                            // short-on-the-other ratio coincidence: if the
                            // sum exceeds the configured maximum thickness,
                            // treat the voxel as outside the band rather
                            // than letting two non-comparable walks pass the
                            // relative test.
                            if d_plus + d_minus > 2.0 * max_walk_dist {
                                continue;
                            }
                            // Equality threshold combines two terms:
                            //  - relative `distance_tolerance × max(d⁺, d⁻)` —
                            //    the PRD's "~5%" rule, sharply discriminates
                            //    centerline voxels when the medial axis aligns
                            //    with a voxel.
                            //  - absolute one-voxel slack `min_spacing` —
                            //    handles voxelized medial axes that fall
                            //    *between* grid points (the analytic medial of
                            //    an axis-aligned slab on an even-N grid lies on
                            //    a half-voxel boundary, so the two closest
                            //    voxels see a one-voxel asymmetry in d⁺/d⁻ —
                            //    namely `min_spacing` — that the strict
                            //    relative rule would reject). The slack is
                            //    therefore exactly `min_spacing`, NOT
                            //    `½·min_spacing`. See the doc comment on
                            //    `MedialOptions::distance_tolerance` for the
                            //    discriminative-regime caveat.
                            let abs_diff = (d_plus - d_minus).abs();
                            let equality_threshold = dist_tol * dmax + min_spacing;
                            if abs_diff < equality_threshold {
                                local.push([i as i32, j as i32, k as i32]);
                            }
                        }
                    }
                }
                local
            }));
        }
        let mut acc: Vec<[i32; 3]> = Vec::new();
        for h in handles {
            match h.join() {
                Ok(local) => acc.extend(local),
                Err(payload) => std::panic::resume_unwind(payload),
            }
        }
        acc
    });
    // Normalise to strict lex order regardless of chunk distribution.
    // sort_unstable is safe: no duplicates (each voxel owned by one thread).
    //
    // In debug builds, assert that the parallel merge already produced
    // near-lex-sorted output — a regression here (e.g. chunk ordering changes
    // without a corresponding sort fix) would surface immediately.
    debug_assert!(
        voxels.windows(2).all(|w| w[0] <= w[1]),
        "parallel merge should already produce lex-sorted output; \
         if this fires, chunk ordering or join order changed"
    );
    voxels.sort_unstable();

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

/// Check that `data_len == nx * ny * nz`; see [`MedialError::AxisExtentsOverflow`]
/// for the overflow path and [`MedialError::DataLengthMismatch`] for the mismatch path.
fn validate_flat_data_length(
    nx: usize,
    ny: usize,
    nz: usize,
    data_len: usize,
) -> Result<(), MedialError> {
    let expected = nx
        .checked_mul(ny)
        .and_then(|p| p.checked_mul(nz))
        .ok_or(MedialError::AxisExtentsOverflow { nx, ny, nz })?;
    if data_len != expected {
        return Err(MedialError::DataLengthMismatch {
            expected,
            found: data_len,
        });
    }
    Ok(())
}

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

/// Pre-compute a dense gradient grid: one `[f64; 3]` entry per voxel,
/// laid out row-major as `grid[i*ny*nz + j*nz + k]` matching the
/// `SampledField::data` convention.
///
/// **Gated semantics.** The layout is still dense (one slot per voxel;
/// O(1) lookup via `i*ny*nz + j*nz + k`).
///
/// - **In-band slots** (`|φ(v)| ≤ band_width`) equal
///   `gradient_at_index(sdf, [i, j, k])` exactly (bit-for-bit, same
///   invariant as before).
/// - **Out-of-band slots** (`|φ(v)| > band_width`) are the `[0.0; 3]`
///   sentinel — the consumer's narrow-band filter in `compute_medial_mask`
///   rejects these voxels BEFORE indexing into the cache, so the sentinel
///   is never read by downstream logic.
///
/// **Cost rationale.** Each out-of-band voxel costs one extra
/// `sample_at_index` call (a single array index) and skips the 6-sample
/// central-difference stencil (`gradient_at_index`). In typical fixtures
/// ~95% of voxels are out-of-band, so the gate yields a substantial
/// saving.
///
/// Note: the consumer (`compute_medial_mask`) also calls `sample_at_index`
/// once per voxel to obtain φ for its own narrow-band filter — so the
/// producer's gate call is duplicated work. The duplication is negligible
/// while `sample_at_index` is a plain array index (O(1)); if
/// `SampledField::data` were ever replaced with a non-trivial sampler
/// (e.g. interpolated or coordinate-transformed), the producer should be
/// refactored to return per-voxel `(phi, gradient)` pairs so the consumer
/// can reuse φ without re-sampling.
///
/// Pass `f64::INFINITY` to disable the gate and compute every slot
/// (useful for testing the strict-equality contract on all voxels).
///
/// **Index formula.** The formula `i*nj*nk + j*nk + k` is identical to
/// [`sample_at_index`]'s layout so cache reads in the main loop are
/// cache-line-friendly for the innermost (k) sweep.
///
/// Memory cost at 256³ is ~384 MB (16 M voxels × 24 B); flagged for
/// replacement with a sparse representation once the OpenVDB FFI lands
/// and `narrow_band_half_width_voxels` becomes a true sparse-iterator
/// gate (the dense-grid gate here is the mitigation until then).
pub(crate) fn precompute_gradient_grid(sdf: &SampledField, band_width: f64) -> Vec<[f64; 3]> {
    let nx = sdf.axis_grids[0].len();
    let ny = sdf.axis_grids[1].len();
    let nz = sdf.axis_grids[2].len();

    // Parallel construction via std::thread::scope + slice::chunks_mut.
    //
    // Each thread receives an exclusive mutable sub-slice of `grid` via
    // `chunks_mut` and writes directly — no transient per-thread Vec, no
    // copy_from_slice. This halves peak memory relative to a build-then-copy
    // approach (from ~768 MB to ~384 MB at 256³).
    //
    // `i_indices.chunks(chunk_size)` and `grid.chunks_mut(chunk_size*ny*nz)`
    // are zipped: the k-th i-chunk pairs with the k-th sub-slice, which are
    // non-overlapping by the guarantee of `chunks_mut`. Each `dst` borrow is
    // valid for the scope lifetime (≤ lifetime of `grid`), and
    // `&mut [[f64;3]]: Send` (since `[f64;3]: Send + Sync`).
    //
    // Determinism: handles joined in spawn order = chunk-iteration order =
    // ascending i order; writes target non-overlapping slices. Per Task-2544:
    // panics forwarded via `resume_unwind` so the original payload reaches
    // the caller intact (contract-explicitness convention).
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(nx.max(1));
    let chunk_size = nx.div_ceil(threads).max(1);
    let i_indices: Vec<usize> = (0..nx).collect();

    let mut grid = vec![[0.0f64; 3]; nx * ny * nz];

    std::thread::scope(|s| {
        let mut handles = Vec::with_capacity(threads);
        for (chunk, dst) in i_indices
            .chunks(chunk_size)
            .zip(grid.chunks_mut(chunk_size * ny * nz))
        {
            handles.push(s.spawn(move || {
                for (idx, &i) in chunk.iter().enumerate() {
                    for j in 0..ny {
                        for k in 0..nz {
                            if sample_at_index(sdf, [i, j, k]).abs() <= band_width {
                                dst[idx * ny * nz + j * nz + k] = gradient_at_index(sdf, [i, j, k]);
                            }
                            // else: out-of-band; slot stays at the [0.0; 3]
                            // sentinel from the initial vec! allocation.
                            // The consumer rejects out-of-band voxels at
                            // line 470 before reading the cache (line 475),
                            // so the sentinel is structurally unreachable
                            // from downstream logic.
                        }
                    }
                }
            }));
        }
        for h in handles {
            match h.join() {
                Ok(()) => {}
                Err(p) => std::panic::resume_unwind(p),
            }
        }
    });

    grid
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
    use crate::grid_validation::GridValidationError;
    use reify_ir::value::{InterpolationKind, SampledGridKind};
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

        // Reach the error type and wrapper variant from the crate root too
        // — sanity-checks that `MedialError` and `GridValidationError` are
        // publicly named.
        let _: MedialError =
            MedialError::GridValidation(GridValidationError::EmptyAxisGrid { axis: 0 });
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
            MedialError::GridValidation(GridValidationError::UnsupportedGridKind {
                found: SampledGridKind::Regular1D,
            })
        );
    }

    #[test]
    fn compute_medial_mask_rejects_regular2d_grids() {
        let sdf = two_d_field();
        let err = compute_medial_mask(&sdf, &MedialOptions::default())
            .expect_err("2D input must be rejected");
        assert_eq!(
            err,
            MedialError::GridValidation(GridValidationError::UnsupportedGridKind {
                found: SampledGridKind::Regular2D,
            })
        );
    }

    /// Floor on `compute_medial_mask(&slab_sdf_3d(3.0, 16), …).voxels.len()`.
    ///
    /// Derived from `n * n / 2` for the 16×16 centerline plane: the medial
    /// algorithm is expected to flag at least half the voxels on that plane
    /// (lower bound, not exact count). For `n = 16`: `16 * 16 / 2 = 128`.
    const SLAB_16_MIN_MEDIAL_VOXELS: usize = 128;

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

        let axis_grid: Vec<f64> = (0..n).map(|i| bounds_min + (i as f64) * spacing).collect();
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
            name: format!("slab-3d-h{half_thickness_voxels}-n{voxel_count}"),
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
    ///     (`mask.voxels.len() ≥ SLAB_16_MIN_MEDIAL_VOXELS`).
    #[test]
    fn compute_medial_mask_flags_slab_centerline_voxels() {
        let n = 16usize;
        let sdf = slab_sdf_3d(3.0, n);
        let mask =
            compute_medial_mask(&sdf, &MedialOptions::default()).expect("slab compute succeeds");

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
        let min_expected = SLAB_16_MIN_MEDIAL_VOXELS;
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

        let axis_grid: Vec<f64> = (0..n).map(|i| bounds_min + (i as f64) * spacing).collect();
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
            name: format!("slab-x-3d-h{half_thickness_voxels}-n{voxel_count}"),
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
        let mask =
            compute_medial_mask(&sdf, &MedialOptions::default()).expect("x-slab compute succeeds");

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

        let axis_grid: Vec<f64> = (0..n).map(|i| bounds_min + (i as f64) * spacing).collect();
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
        let mask =
            compute_medial_mask(&sdf, &MedialOptions::default()).expect("sphere compute succeeds");

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
        assert!(
            voxel_count >= 2,
            "thick block grid needs ≥ 2 voxels per axis"
        );
        let n = voxel_count;
        let spacing: f64 = 1.0;
        let half_extent = (n as f64 - 1.0) / 2.0;
        let bounds_min = -half_extent;
        let bounds_max = half_extent;

        let axis_grid: Vec<f64> = (0..n).map(|i| bounds_min + (i as f64) * spacing).collect();
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
            bounds_min: vec![0.0],           // length 1 — mismatch
            bounds_max: vec![0.0, 0.0, 0.0], // length 3
            spacing: vec![1.0, 1.0, 1.0],    // length 3
            axis_grids: vec![vec![0.0], vec![0.0], vec![0.0]], // length 3
            interpolation: InterpolationKind::Linear,
            data: vec![1.0],
            oob_emitted: AtomicBool::new(false),
        };
        let err = compute_medial_mask(&sdf, &MedialOptions::default())
            .expect_err("axis-length-mismatch input must be rejected");
        assert_eq!(
            err,
            MedialError::GridValidation(GridValidationError::AxisLengthMismatch {
                bounds_min_len: 1,
                bounds_max_len: 3,
                spacing_len: 3,
                axis_grids_len: 3,
            })
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
        assert_eq!(
            err,
            MedialError::GridValidation(GridValidationError::EmptyAxisGrid { axis: 0 })
        );
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
        let sdf = geometry_test_field([0.0, 1.0, 1.0], [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
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
        let sdf = geometry_test_field([-1.0, 1.0, 1.0], [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
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
        let sdf = geometry_test_field([f64::NAN, 1.0, 1.0], [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
        let err = compute_medial_mask(&sdf, &MedialOptions::default())
            .expect_err("NaN spacing must be rejected");
        match err {
            MedialError::InvalidAxisGeometry {
                axis: 0, spacing, ..
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

    /// Contract test: `compute_medial_mask` must return voxels in strict
    /// lexicographic ascending order (element-wise `[i32; 3]` comparison).
    ///
    /// **Why pin this now?** The serial triple-nested loop (i → j → k)
    /// produces lex-sorted output by construction; downstream consumers
    /// (e.g. binary-search-style mask queries) already rely on this.
    /// Pinning the contract here ensures that step-6's parallel
    /// chunk-merge + `sort_unstable` cannot silently regress it: if the
    /// final sort is ever accidentally removed, or the per-chunk Vecs are
    /// concatenated in a non-deterministic order, this test will catch the
    /// regression before any downstream consumer is affected.
    ///
    /// The `windows(2).all(|w| w[0] < w[1])` check enforces *strict*
    /// ordering (no duplicates); each voxel is visited at most once so
    /// equality would indicate a bug in the parallelisation logic.
    #[test]
    fn compute_medial_mask_voxels_are_sorted_in_lex_order_on_slab() {
        let sdf = slab_sdf_3d(3.0, 16);
        let mask =
            compute_medial_mask(&sdf, &MedialOptions::default()).expect("slab compute succeeds");

        // The slab fixture produces at least `SLAB_16_MIN_MEDIAL_VOXELS` medial
        // voxels (see `compute_medial_mask_flags_slab_centerline_voxels` part (c)),
        // so the windows(2) ordering check is load-bearing.
        assert!(
            mask.voxels.len() >= SLAB_16_MIN_MEDIAL_VOXELS,
            "need ≥ {SLAB_16_MIN_MEDIAL_VOXELS} voxels for ordering check; got {}",
            mask.voxels.len()
        );

        let out_of_order = mask.voxels.windows(2).find(|w| w[0] >= w[1]);
        assert!(
            out_of_order.is_none(),
            "medial mask voxels must be strictly lex-ordered; \
             found {:?} ≥ {:?}",
            out_of_order.unwrap()[0],
            out_of_order.unwrap()[1]
        );
    }

    /// Contract test: `compute_medial_mask` must return bit-identical output
    /// across ≥3 independent sequential calls on the same input.
    ///
    /// **Why ≥3 runs?** Two in-process runs share scheduler state and a true
    /// non-determinism bug (e.g., unsorted parallel-chunk merge, missing
    /// `sort_unstable`) can pass by luck when both runs happen to produce the
    /// same ordering. Three independent sequential runs make scheduler-state
    /// coincidence strictly less likely than two runs do; we choose 3 as a
    /// small constant that meaningfully reduces the false-pass surface without
    /// significantly inflating CI time.
    ///
    /// **Why both fixture sizes?** 16³ gives fast CI feedback; at this size
    /// with `available_parallelism() ≥ 4` each chunk covers only 4 i-rows —
    /// per-chunk runtimes are nearly identical, limiting scheduling variance.
    /// 48³ gives the OS scheduler 12× more per-chunk work, exposing chunk-
    /// interleaving opportunities that 16³ would miss. Both are needed: 16³
    /// catches constant-factor bugs; 48³ catches variance-dependent bugs.
    ///
    /// **Equality semantics.** Each subsequent run is compared against run 0
    /// (not all-pairs). Transitive equality holds: if run 1 == run 0 and
    /// run 2 == run 0, then run 1 == run 2 — so comparing against run 0 is
    /// sufficient and has linear cost in the run count.
    ///
    /// Checks `voxels` (full Vec including ordering), `spacing`, and `origin`.
    #[test]
    fn compute_medial_mask_is_deterministic_across_three_runs_on_multiple_slab_sizes() {
        let opts = MedialOptions::default();
        for n in [16usize, 48usize] {
            let sdf = slab_sdf_3d(3.0, n);
            let runs: Vec<MedialMask> = (0..3)
                .map(|run| {
                    compute_medial_mask(&sdf, &opts)
                        .unwrap_or_else(|e| panic!("run {run} on n={n} failed: {e:?}"))
                })
                .collect();
            for (run_idx, run) in runs.iter().enumerate().skip(1) {
                assert_eq!(
                    run.voxels, runs[0].voxels,
                    "n={n} run={run_idx}: voxels differ from run 0"
                );
                assert_eq!(
                    run.spacing, runs[0].spacing,
                    "n={n} run={run_idx}: spacing differs from run 0"
                );
                assert_eq!(
                    run.origin, runs[0].origin,
                    "n={n} run={run_idx}: origin differs from run 0"
                );
            }
        }
    }

    /// Contract test: `precompute_gradient_grid` must return a flat Vec
    /// whose entry at `i*ny*nz + j*nz + k` equals
    /// `gradient_at_index(sdf, [i, j, k])` exactly (bit-for-bit, not
    /// approximately).
    ///
    /// **Why exact equality?** The cache MUST be a faithful precomputation,
    /// not a numerically-different approximation. If the cached gradient
    /// differs from the inline `gradient_at_index` call — even by a single
    /// ULP — the parallel medial-mask impl would make different
    /// inclusion/exclusion decisions than the serial reference, silently
    /// changing the medial mask. The helper is a verbatim hoist of the
    /// same computation, so exact `==` is the correct invariant.
    ///
    /// Uses `f64::INFINITY` to disable the narrow-band gate so the
    /// strict-equality contract is exercised on every voxel; the gated
    /// behavior is covered separately by
    /// `precompute_gradient_grid_skips_out_of_band_voxels`.
    #[test]
    fn precompute_gradient_grid_matches_gradient_at_index_on_slab_when_gate_disabled() {
        let n = 16usize;
        let sdf = slab_sdf_3d(3.0, n);
        let ny = sdf.axis_grids[1].len();
        let nz = sdf.axis_grids[2].len();

        let grid = precompute_gradient_grid(&sdf, f64::INFINITY);

        assert_eq!(
            grid.len(),
            n * ny * nz,
            "gradient grid length must be nx*ny*nz"
        );

        for i in 0..n {
            for j in 0..ny {
                for k in 0..nz {
                    let expected = gradient_at_index(&sdf, [i, j, k]);
                    let got = grid[i * ny * nz + j * nz + k];
                    assert_eq!(
                        got, expected,
                        "gradient mismatch at ({i},{j},{k}): \
                         cache={got:?}, inline={expected:?}"
                    );
                }
            }
        }
    }

    /// Contract test: `precompute_gradient_grid` with a finite `band_width`
    /// must only fill in the central-difference gradient for *in-band* voxels
    /// (`|φ(v)| ≤ band_width`) and leave out-of-band slots at the `[0.0; 3]`
    /// sentinel from the initial allocation.
    ///
    /// **Why both branches matter.**
    /// *In-band correctness:* the producer-side gate and the consumer-side gate
    /// in `compute_medial_mask` (line 470) both compare
    /// `sample_at_index(sdf, [i, j, k]).abs() <= band_width`. Pinning that
    /// in-band slots equal `gradient_at_index` exactly ensures there is no
    /// FP-rounding skew: the producer never skips a voxel the consumer
    /// would have read.
    /// *Out-of-band sentinel:* the consumer rejects out-of-band voxels at
    /// line 470 *before* indexing into the cache (line 475), so the `[0.0; 3]`
    /// sentinel is structurally unreachable from downstream logic.  Pinning
    /// the sentinel confirms that skipping the 6-sample stencil for out-of-band
    /// voxels is safe.
    ///
    /// The fixture uses `slab_sdf_3d(3.0, 16)` (z ∈ −7.5..7.5, φ = |z| − 3)
    /// with `band_width = 1.0` so voxels with |φ| ≤ 1.0 (z ≈ ±2.5, ±3.5)
    /// form a non-empty in-band partition and the rest form a non-empty
    /// out-of-band partition — both branches are exercised.
    #[test]
    fn precompute_gradient_grid_skips_out_of_band_voxels() {
        let sdf = slab_sdf_3d(3.0, 16);
        let ny = sdf.axis_grids[1].len();
        let nz = sdf.axis_grids[2].len();
        let nx = sdf.axis_grids[0].len();

        let band_width = 1.0_f64;
        let grid = precompute_gradient_grid(&sdf, band_width);

        assert_eq!(
            grid.len(),
            nx * ny * nz,
            "gradient grid length must be nx*ny*nz"
        );

        let mut in_band_count = 0usize;
        let mut out_of_band_count = 0usize;

        for i in 0..nx {
            for j in 0..ny {
                for k in 0..nz {
                    let phi = sample_at_index(&sdf, [i, j, k]);
                    let slot = grid[i * ny * nz + j * nz + k];
                    if phi.abs() <= band_width {
                        in_band_count += 1;
                        let expected = gradient_at_index(&sdf, [i, j, k]);
                        assert_eq!(
                            slot, expected,
                            "in-band gradient mismatch at ({i},{j},{k}): \
                             cache={slot:?}, inline={expected:?}"
                        );
                    } else {
                        out_of_band_count += 1;
                        assert_eq!(
                            slot, [0.0_f64; 3],
                            "out-of-band slot at ({i},{j},{k}) should be \
                             sentinel [0.0; 3], got {slot:?}"
                        );
                    }
                }
            }
        }

        assert!(
            in_band_count > 0,
            "fixture must have at least one in-band voxel \
             (band_width={band_width})"
        );
        assert!(
            out_of_band_count > 0,
            "fixture must have at least one out-of-band voxel \
             (band_width={band_width})"
        );
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

    /// Characterization — asymmetric-extents DataLengthMismatch: 3×4×5 grid
    /// (nx*ny*nz = 60) with only 59 data elements.
    ///
    /// Pins (a) the validator handles non-cubic grids (axis lengths differ on
    /// each axis) and (b) `validate_flat_data_length`'s checked_mul math is
    /// exercised on asymmetric extents through the public API, not just the
    /// unit test from step-5. The existing test at line 1628 uses a 2×2×2
    /// cube; this complements it with an asymmetric case.
    #[test]
    fn compute_medial_mask_rejects_data_length_mismatch_on_asymmetric_extents() {
        let sdf = SampledField {
            name: "test-asymmetric-3x4x5".to_string(),
            kind: SampledGridKind::Regular3D,
            bounds_min: vec![0.0, 0.0, 0.0],
            bounds_max: vec![2.0, 3.0, 4.0],
            spacing: vec![1.0, 1.0, 1.0],
            axis_grids: vec![
                vec![0.0, 1.0, 2.0],           // nx = 3
                vec![0.0, 1.0, 2.0, 3.0],      // ny = 4
                vec![0.0, 1.0, 2.0, 3.0, 4.0], // nz = 5
            ],
            interpolation: InterpolationKind::Linear,
            data: vec![0.0; 59], // should be 60 (3*4*5)
            oob_emitted: AtomicBool::new(false),
        };
        let err = compute_medial_mask(&sdf, &MedialOptions::default())
            .expect_err("3×4×5 grid with 59 data elements must be rejected");
        assert_eq!(
            err,
            MedialError::DataLengthMismatch {
                expected: 60,
                found: 59,
            },
            "validate_flat_data_length must compute 3*4*5=60 and report found=59"
        );
    }

    /// Characterization — +Inf spacing on axis 0 is rejected via `is_finite()`.
    ///
    /// Pins the implicit behavior that +Inf spacing (not just NaN/zero/negative)
    /// is caught by the existing `sp.is_finite() && sp > 0.0` predicate.
    /// Future refactors of the geometry-validation block must not silently
    /// regress ±Inf handling.
    #[test]
    fn compute_medial_mask_rejects_positive_infinity_spacing() {
        let sdf = geometry_test_field([f64::INFINITY, 1.0, 1.0], [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
        let err = compute_medial_mask(&sdf, &MedialOptions::default())
            .expect_err("+Inf spacing must be rejected");
        assert_eq!(
            err,
            MedialError::InvalidAxisGeometry {
                axis: 0,
                spacing: f64::INFINITY,
                bounds_min: 0.0,
                bounds_max: 1.0,
            }
        );
    }

    /// Characterization — -Inf spacing on axis 0 is rejected via `is_finite()`.
    #[test]
    fn compute_medial_mask_rejects_negative_infinity_spacing() {
        let sdf = geometry_test_field(
            [f64::NEG_INFINITY, 1.0, 1.0],
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
        );
        let err = compute_medial_mask(&sdf, &MedialOptions::default())
            .expect_err("-Inf spacing must be rejected");
        assert_eq!(
            err,
            MedialError::InvalidAxisGeometry {
                axis: 0,
                spacing: f64::NEG_INFINITY,
                bounds_min: 0.0,
                bounds_max: 1.0,
            }
        );
    }

    /// Characterization — +Inf bounds_max on axis 0 is rejected via `bmax.is_finite()`.
    #[test]
    fn compute_medial_mask_rejects_positive_infinity_bounds_max() {
        let sdf = geometry_test_field([1.0, 1.0, 1.0], [0.0, 0.0, 0.0], [f64::INFINITY, 1.0, 1.0]);
        let err = compute_medial_mask(&sdf, &MedialOptions::default())
            .expect_err("+Inf bounds_max must be rejected");
        assert_eq!(
            err,
            MedialError::InvalidAxisGeometry {
                axis: 0,
                spacing: 1.0,
                bounds_min: 0.0,
                bounds_max: f64::INFINITY,
            }
        );
    }

    /// Characterization — -Inf bounds_min on axis 0 is rejected via `bmin.is_finite()`.
    #[test]
    fn compute_medial_mask_rejects_negative_infinity_bounds_min() {
        let sdf = geometry_test_field(
            [1.0, 1.0, 1.0],
            [f64::NEG_INFINITY, 0.0, 0.0],
            [1.0, 1.0, 1.0],
        );
        let err = compute_medial_mask(&sdf, &MedialOptions::default())
            .expect_err("-Inf bounds_min must be rejected");
        assert_eq!(
            err,
            MedialError::InvalidAxisGeometry {
                axis: 0,
                spacing: 1.0,
                bounds_min: f64::NEG_INFINITY,
                bounds_max: 1.0,
            }
        );
    }

    /// Characterization — zero spacing on axis 1 is rejected with `axis: 1`.
    ///
    /// Pins that the validator's `for axis in 0..3` loop reports the correct
    /// non-zero axis index, not always 0. A future refactor that collapsed the
    /// loop to axis-0-only would silently miss axis-1/2 violations.
    #[test]
    fn compute_medial_mask_rejects_zero_spacing_on_axis_1() {
        let sdf = geometry_test_field(
            [1.0, 0.0, 1.0], // axis-1 spacing = 0
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
        );
        let err = compute_medial_mask(&sdf, &MedialOptions::default())
            .expect_err("zero spacing on axis 1 must be rejected");
        assert_eq!(
            err,
            MedialError::InvalidAxisGeometry {
                axis: 1,
                spacing: 0.0,
                bounds_min: 0.0,
                bounds_max: 1.0,
            }
        );
    }

    /// Characterization — inverted bounds on axis 2 are rejected with `axis: 2`.
    ///
    /// Pins that the `for axis in 0..3` loop correctly identifies the first
    /// violating axis when it is axis 2. bounds_min[2]=1.0 > bounds_max[2]=0.0.
    #[test]
    fn compute_medial_mask_rejects_inverted_bounds_on_axis_2() {
        let sdf = geometry_test_field(
            [1.0, 1.0, 1.0],
            [0.0, 0.0, 1.0], // bounds_min[2] = 1.0 > bounds_max[2] = 0.0
            [1.0, 1.0, 0.0],
        );
        let err = compute_medial_mask(&sdf, &MedialOptions::default())
            .expect_err("inverted bounds on axis 2 must be rejected");
        assert_eq!(
            err,
            MedialError::InvalidAxisGeometry {
                axis: 2,
                spacing: 1.0,
                bounds_min: 1.0,
                bounds_max: 0.0,
            }
        );
    }

    /// Unit test for `validate_flat_data_length`: exercises all three result
    /// branches (Ok, AxisExtentsOverflow, DataLengthMismatch) without
    /// constructing a full SampledField — keeping the test fast and avoiding
    /// the memory pressure needed to trigger a real overflow through the
    /// public API.
    #[test]
    fn validate_flat_data_length_routes_overflow_and_mismatch() {
        // (a) Ok on matching inputs: 2×3×4 = 24.
        validate_flat_data_length(2, 3, 4, 24).expect("ok on consistent inputs");

        // (b) AxisExtentsOverflow when the product overflows usize.
        // 2^22 = 4_194_304; cubed = 2^66 which overflows both 32- and 64-bit usize.
        let n = 1usize << 22;
        match validate_flat_data_length(n, n, n, 0) {
            Err(MedialError::AxisExtentsOverflow { nx, ny, nz }) => {
                assert_eq!(nx, n, "AxisExtentsOverflow must carry nx");
                assert_eq!(ny, n, "AxisExtentsOverflow must carry ny");
                assert_eq!(nz, n, "AxisExtentsOverflow must carry nz");
            }
            other => panic!("expected AxisExtentsOverflow, got {other:?}"),
        }

        // (c) DataLengthMismatch when the product is valid but data_len differs.
        assert_eq!(
            validate_flat_data_length(2, 3, 4, 23),
            Err(MedialError::DataLengthMismatch {
                expected: 24,
                found: 23,
            }),
            "DataLengthMismatch must carry expected=24 and found=23"
        );
    }

    /// Display data-flow test: `MedialError::AxisExtentsOverflow` must include
    /// each of the three extent values (nx, ny, nz) in the formatted message,
    /// verifying that the variant fields are surfaced to the user.
    #[test]
    fn axis_extents_overflow_display_includes_extent_values() {
        let err = MedialError::AxisExtentsOverflow {
            nx: 3_000_000,
            ny: 4_000_000,
            nz: 5_000_000,
        };
        let msg = format!("{err}");
        assert!(
            msg.contains("3000000"),
            "AxisExtentsOverflow Display must include nx=3000000: {msg}"
        );
        assert!(
            msg.contains("4000000"),
            "AxisExtentsOverflow Display must include ny=4000000: {msg}"
        );
        assert!(
            msg.contains("5000000"),
            "AxisExtentsOverflow Display must include nz=5000000: {msg}"
        );
    }

    // ── δ=4424 step-1: min_wall_thickness RED tests ──────────────────────────
    //
    // Both tests below reference `min_wall_thickness` and `MinWallThickness`
    // which do NOT yet exist in medial.rs — they compile-fail (RED) until
    // step-2 adds the implementation.

    /// Build an analytic slab Regular3D `SampledField` representing
    /// `φ(x, y, z) = |z| − thickness_mm/2` over an `n³` grid centered on
    /// the origin with physical spacing `h` (mm). Thin in z, spanning x/y —
    /// the "2mm box" analytic fixture for δ (task 4424) min-wall tests.
    /// Mirrors the structure of `slab_sdf_3d` but parameterised in physical
    /// units (mm) rather than unit-spacing voxels.
    fn analytic_slab_box(thickness_mm: f64, h: f64, n: usize) -> SampledField {
        assert!(n >= 2, "slab grid needs ≥ 2 voxels per axis");
        let half_thickness = thickness_mm / 2.0;
        let half_extent = (n as f64 - 1.0) / 2.0 * h;
        let bounds_min = -half_extent;
        let bounds_max = half_extent;

        let axis_grid: Vec<f64> =
            (0..n).map(|i| bounds_min + (i as f64) * h).collect();
        let mut data = Vec::with_capacity(n * n * n);
        for &_x in &axis_grid {
            for &_y in &axis_grid {
                for &z in &axis_grid {
                    data.push(z.abs() - half_thickness);
                }
            }
        }
        SampledField {
            name: format!("slab-box-{thickness_mm}mm-h{h}-n{n}"),
            kind: SampledGridKind::Regular3D,
            bounds_min: vec![bounds_min, bounds_min, bounds_min],
            bounds_max: vec![bounds_max, bounds_max, bounds_max],
            spacing: vec![h, h, h],
            axis_grids: vec![axis_grid.clone(), axis_grid.clone(), axis_grid],
            interpolation: InterpolationKind::Linear,
            data,
            oob_emitted: AtomicBool::new(false),
        }
    }

    /// Build an L-bracket Regular3D `SampledField` as the union (minimum) of
    /// two perpendicular analytic slabs:
    ///   `φ(x, y, z) = min(|z| − web_mm/2, |x| − web_mm/2)`
    /// over an `n³` grid centered on the origin with physical spacing `h` (mm).
    /// D4 fixture for the min_wall_thickness conservative-bound gate (§9 Q4).
    fn analytic_l_bracket(web_mm: f64, h: f64, n: usize) -> SampledField {
        assert!(n >= 2, "L-bracket grid needs ≥ 2 voxels per axis");
        let half_web = web_mm / 2.0;
        let half_extent = (n as f64 - 1.0) / 2.0 * h;
        let bounds_min = -half_extent;
        let bounds_max = half_extent;

        let axis_grid: Vec<f64> =
            (0..n).map(|i| bounds_min + (i as f64) * h).collect();
        let mut data = Vec::with_capacity(n * n * n);
        for &x in &axis_grid {
            for &_y in &axis_grid {
                for &z in &axis_grid {
                    let phi_z = z.abs() - half_web; // horizontal slab
                    let phi_x = x.abs() - half_web; // vertical slab
                    data.push(phi_z.min(phi_x)); // union = minimum SDF
                }
            }
        }
        SampledField {
            name: format!("l-bracket-{web_mm}mm-h{h}-n{n}"),
            kind: SampledGridKind::Regular3D,
            bounds_min: vec![bounds_min, bounds_min, bounds_min],
            bounds_max: vec![bounds_max, bounds_max, bounds_max],
            spacing: vec![h, h, h],
            axis_grids: vec![axis_grid.clone(), axis_grid.clone(), axis_grid],
            interpolation: InterpolationKind::Linear,
            data,
            oob_emitted: AtomicBool::new(false),
        }
    }

    /// PRD §3b / δ=4424 step-1a:
    /// `min_wall_thickness` for a 2.0mm analytic slab SDF at spacing h=0.5mm
    /// must:
    ///   (a) return `Ok(Measured(t))` — slab is above the 2h floor.
    ///   (b) `|t − 2.0| ≤ h` — within one voxel of the analytic thickness.
    ///   (c) `t ≤ 2.0 + h` — conservative-lower-bound: min-reduction cannot
    ///       OVERestimate.
    ///
    /// G6 honest-floor: inequalities only, NO exact float, NO machine-epsilon.
    /// φ = |z|−1.0 is piecewise-linear ⇒ walk_to_zero exact ⇒ d⁺=d⁻=1.0 ⇒
    /// d⁺+d⁻=2.0mm at every medial voxel ⇒ both bounds hold by construction.
    #[test]
    fn min_wall_thickness_box_2mm_is_conservative_lower_bound() {
        let h = 0.5_f64;
        let sdf = analytic_slab_box(2.0, h, 12);
        let result =
            min_wall_thickness(&sdf, h).expect("valid analytic slab must not error");
        match result {
            MinWallThickness::Measured(t) => {
                assert!(
                    (t - 2.0).abs() <= h,
                    "2mm slab: |t−2.0|={} must be ≤ h={}; got t={t}",
                    (t - 2.0).abs(),
                    h
                );
                assert!(
                    t <= 2.0 + h,
                    "2mm slab: t={t} must be ≤ 2.0+h={} (conservative lower bound)",
                    2.0 + h
                );
            }
            other => panic!(
                "expected MinWallThickness::Measured(_) for 2mm slab at h={h}, \
                 got {other:?}"
            ),
        }
    }

    /// PRD §3b D4 gate / δ=4424 step-1b — §9 Q4:
    /// For an L-bracket SDF (union/min of two perpendicular slabs, web thickness
    /// w=2.0mm), if `min_wall_thickness` returns `Measured(t)`, then `t ≤ w+h`.
    ///
    /// walk_to_zero returns the FIRST zero-crossing (nearest surface) so a
    /// per-voxel d⁺+d⁻ cannot OVERestimate; the min-reduction can only
    /// UNDERestimate.  The clean leg centres are locally slabs that register
    /// medial voxels (proven by the slab test above).
    ///
    /// `NoMeasurement` is accepted as a valid fall-through if the union mask
    /// does not register (D4 §9-Q4 scope decision: restrict to convex-ish in
    /// that case and file non-convex correctness as a follow-up).
    #[test]
    fn min_wall_thickness_l_bracket_conservative_bound_holds() {
        let w = 2.0_f64;
        let h = 0.5_f64;
        let sdf = analytic_l_bracket(w, h, 12);
        let result =
            min_wall_thickness(&sdf, h).expect("valid L-bracket field must not error");
        // w=2.0mm ≥ 2h=1.0mm, so BelowResolution would be a regression.
        match result {
            MinWallThickness::Measured(t) => {
                assert!(
                    t <= w + h,
                    "L-bracket Measured({t}) must be ≤ w+h={} \
                     (conservative lower bound on re-entrant geometry)",
                    w + h
                );
            }
            MinWallThickness::NoMeasurement => {
                // Accepted: D4 §9-Q4 fall-through — union mask may not register
                // medial voxels on non-convex geometry; restrict to convex-ish.
            }
            MinWallThickness::BelowResolution { raw, floor } => {
                panic!(
                    "L-bracket with w={w}mm web (≥ 2h={:.1}mm) must NOT return \
                     BelowResolution — the web is comfortably above the 2h floor; \
                     got BelowResolution {{ raw: {raw}, floor: {floor} }}",
                    2.0 * h,
                );
            }
        }
    }

    // ── δ=4424 step-3: BelowResolution RED test ──────────────────────────────
    //
    // Under step-2's impl, min_wall_thickness always returns Measured(_) for
    // any finite min-sum. Step-3 adds a test that requires BelowResolution
    // for a sub-2h slab (0.8mm at h=0.5 → 2h=1.0mm). The test is RED until
    // step-4 adds the honest-floor branch.

    /// PRD §3b G6 honest-floor / δ=4424 step-3:
    /// `min_wall_thickness` for a 0.8mm analytic slab at h=0.5mm (2h=1.0mm)
    /// must return `Ok(BelowResolution { raw, floor })` with `floor == 2·h`
    /// and `raw < floor`.
    ///
    /// The feature must be REPORTED self-describingly — NEVER silently returned
    /// as `Measured` (which would imply the measurement is reliable at this
    /// resolution). This structurally avoids the esc-3453 (guessed %) and
    /// esc-3770 (impossible 1e-12) failure modes.
    ///
    /// Voxel-budget proof: with h=0.5 the grid voxels near the slab centre are
    /// at z=±0.25 (inside the 0.8mm slab, half_thickness=0.4mm). For each such
    /// voxel d⁺+d⁻ = 0.80mm (piecewise-linear SDF ⇒ exact), which is < 2h=1.0.
    /// Under step-2 the function returns Measured(0.80) instead → RED.
    #[test]
    fn min_wall_thickness_below_resolution_feature_is_reported_not_rounded() {
        let h = 0.5_f64;
        // 0.8mm slab: 2h = 1.0mm, true wall 0.8mm < 2h → below resolution.
        let sdf = analytic_slab_box(0.8, h, 12);
        let result =
            min_wall_thickness(&sdf, h).expect("valid sub-2h slab must not error");
        // Must NOT be Measured — below-resolution features must not be silently
        // promoted to a seemingly-reliable thickness value.
        assert!(
            !matches!(result, MinWallThickness::Measured(_)),
            "0.8mm slab at h=0.5 must NOT be Measured (raw is below 2h=1.0mm); \
             got {result:?}"
        );
        // Must be BelowResolution with floor == 2·h and raw < floor.
        match result {
            MinWallThickness::BelowResolution { raw, floor } => {
                assert_eq!(
                    floor,
                    2.0 * h,
                    "BelowResolution floor must be exactly 2·h={}, got {floor}",
                    2.0 * h
                );
                assert!(
                    raw < floor,
                    "BelowResolution raw={raw} must be < floor={floor}"
                );
            }
            other => panic!(
                "expected BelowResolution for 0.8mm slab at h=0.5, got {other:?}"
            ),
        }
    }

    // ── ε=4425 step-1: accuracy + anti-ambiguity RED tests ───────────────────
    //
    // References `min_feature_size_measure` and `MinFeatureSize` (neither
    // exists yet → compile-fail RED). Step-2 adds the enum + fn (no floor),
    // turning these GREEN.

    /// Build a "rib and plate" analytic SampledField:
    ///   `φ(x, y, z) = min(|x| − rib_mm/2, |z| − plate_mm/2)`
    /// over an `n³` grid centered on the origin with physical spacing `h`.
    /// The thin rib is centred on x=0 (half-thickness rib_mm/2 in x), and the
    /// wide plate is centred on z=0 (half-thickness plate_mm/2 in z).
    /// Union (min SDF): interior is rib-OR-plate.
    ///
    /// ε's anti-ambiguity fixture (PRD §9 Q5): `min_feature_size_measure`
    /// must pick the rib (thin), NOT the plate (thick) nor the in-plane face.
    fn analytic_rib_and_plate(rib_mm: f64, plate_mm: f64, h: f64, n: usize) -> SampledField {
        assert!(n >= 2, "rib-and-plate grid needs ≥ 2 voxels per axis");
        let half_rib = rib_mm / 2.0;
        let half_plate = plate_mm / 2.0;
        let half_extent = (n as f64 - 1.0) / 2.0 * h;
        let bounds_min = -half_extent;
        let bounds_max = half_extent;

        let axis_grid: Vec<f64> =
            (0..n).map(|i| bounds_min + (i as f64) * h).collect();
        let mut data = Vec::with_capacity(n * n * n);
        for &x in &axis_grid {
            for &_y in &axis_grid {
                for &z in &axis_grid {
                    let phi_x = x.abs() - half_rib; // rib: thin in x
                    let phi_z = z.abs() - half_plate; // plate: thick in z
                    data.push(phi_x.min(phi_z)); // union = minimum SDF
                }
            }
        }
        SampledField {
            name: format!("rib-plate-r{rib_mm}-p{plate_mm}-h{h}-n{n}"),
            kind: SampledGridKind::Regular3D,
            bounds_min: vec![bounds_min, bounds_min, bounds_min],
            bounds_max: vec![bounds_max, bounds_max, bounds_max],
            spacing: vec![h, h, h],
            axis_grids: vec![axis_grid.clone(), axis_grid.clone(), axis_grid],
            interpolation: InterpolationKind::Linear,
            data,
            oob_emitted: AtomicBool::new(false),
        }
    }

    /// PRD §3b / ε=4425 step-1b:
    /// `min_feature_size_measure` for a 2.0mm analytic slab SDF at spacing
    /// h=0.5mm must:
    ///   (a) return `Ok(Measured(t))` — slab is above the 2h floor.
    ///   (b) `t ≤ thickness + h` — conservative-lower-bound / biased-low.
    ///   (c) `t ≥ thickness − 2·h` — within the [thickness−2h, thickness+h] band.
    ///
    /// Closed form: even grid ⇒ medial voxels at z=±0.25 ⇒ 2|φ|=2·0.75=1.5
    /// exact ⇒ both bounds hold with margin h (1.5≤2.5 and 1.5≥1.0).
    #[test]
    fn min_feature_size_measure_box_2mm_is_conservative_lower_bound() {
        let h = 0.5_f64;
        let thickness = 2.0_f64;
        let sdf = analytic_slab_box(thickness, h, 12);
        let result =
            min_feature_size_measure(&sdf, h).expect("valid analytic slab must not error");
        match result {
            MinFeatureSize::Measured(t) => {
                assert!(
                    t <= thickness + h,
                    "2mm slab: t={t} must be ≤ thickness+h={} (conservative lower bound)",
                    thickness + h
                );
                assert!(
                    t >= thickness - 2.0 * h,
                    "2mm slab: t={t} must be ≥ thickness−2h={} (within band)",
                    thickness - 2.0 * h
                );
            }
            other => panic!(
                "expected MinFeatureSize::Measured(_) for 2mm slab at h={h}, \
                 got {other:?}"
            ),
        }
    }

    /// PRD §3b anti-ambiguity / ε=4425 step-1c:
    /// For a rib-and-plate SDF (rib=2.0mm, plate=6.0mm, h=0.5mm),
    /// `min_feature_size_measure` must pick the THIN rib, not the wide plate
    /// nor the in-plane face diameter.
    ///   (a) `t ≤ rib + h` — if the impl wrongly picks the plate (≈5.5) or
    ///       face this bound fails (5.5 > 2.5).
    ///   (b) `t ≥ rib − 2·h` — within the biased-low band for the rib.
    ///
    /// Closed form: rib mid-plane x=±0.25 ⇒ 2|φ|=1.5; plate mid-plane z=±0.25
    /// ⇒ 2|φ|=5.5. min=1.5, rib=2.0: 1.5≤2.5 and 1.5≥1.0 ✓.
    #[test]
    fn min_feature_size_measure_picks_thin_rib_not_wide_face() {
        let h = 0.5_f64;
        let rib = 2.0_f64;
        let sdf = analytic_rib_and_plate(rib, 6.0, h, 16);
        let result =
            min_feature_size_measure(&sdf, h).expect("valid rib-and-plate sdf must not error");
        match result {
            MinFeatureSize::Measured(t) => {
                assert!(
                    t <= rib + h,
                    "rib-and-plate: t={t} must be ≤ rib+h={} (must pick rib, not plate≈5.5)",
                    rib + h
                );
                assert!(
                    t >= rib - 2.0 * h,
                    "rib-and-plate: t={t} must be ≥ rib−2h={} (within biased-low band)",
                    rib - 2.0 * h
                );
            }
            other => panic!(
                "expected MinFeatureSize::Measured(_) for rib-and-plate at h={h}, \
                 got {other:?}"
            ),
        }
    }

    // ── ε=4425 step-3: BelowResolution RED test ──────────────────────────────
    //
    // Under step-2's impl, min_feature_size_measure always returns Measured(_)
    // for any finite min_abs. Step-3 adds a test that requires BelowResolution
    // for a sub-2h slab (0.8mm at h=0.5 → 2h=1.0mm). The test is RED until
    // step-4 adds the honest-floor branch.

    /// PRD §3b G6 honest-floor / ε=4425 step-3:
    /// `min_feature_size_measure` for a 0.8mm analytic slab at h=0.5mm
    /// (2h=1.0mm) must return `Ok(BelowResolution { raw, floor })` with
    /// `floor == 2·h` and `raw < floor`.
    ///
    /// The feature must be REPORTED self-describingly — NEVER silently returned
    /// as `Measured` (which would imply reliability at this resolution).
    /// Mirrors δ step-3 structurally: avoids the esc-3453 (guessed %)
    /// and esc-3770 (impossible 1e-12) failure modes.
    ///
    /// Closed form: medial voxels at z=±0.25 ⇒ |φ|=|0.25−0.4|=0.15 ⇒
    /// 2|φ|=0.3 < 2h=1.0 ⇒ BelowResolution{raw=0.3, floor=1.0}.
    /// Under step-2 the function returns Measured(0.3) → RED.
    #[test]
    fn min_feature_size_measure_below_resolution_feature_is_reported_not_rounded() {
        let h = 0.5_f64;
        // 0.8mm slab: 2h=1.0mm, true feature 0.8mm < 2h → below resolution.
        let sdf = analytic_slab_box(0.8, h, 12);
        let result = min_feature_size_measure(&sdf, h)
            .expect("valid sub-2h slab must not error");
        // Must NOT be Measured — below-resolution features must not be silently
        // promoted to a seemingly-reliable size value.
        assert!(
            !matches!(result, MinFeatureSize::Measured(_)),
            "0.8mm slab at h=0.5 must NOT be Measured (raw is below 2h=1.0mm); \
             got {result:?}"
        );
        // Must be BelowResolution with floor == 2·h and raw < floor.
        match result {
            MinFeatureSize::BelowResolution { raw, floor } => {
                assert_eq!(
                    floor,
                    2.0 * h,
                    "BelowResolution floor must be exactly 2·h={}, got {floor}",
                    2.0 * h
                );
                assert!(
                    raw < floor,
                    "BelowResolution raw={raw} must be < floor={floor}"
                );
            }
            other => panic!(
                "expected BelowResolution for 0.8mm slab at h=0.5, got {other:?}"
            ),
        }
    }
}
