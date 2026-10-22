//! The f32 dense signed-distance sample carrier and the ingest options for
//! implicit solids. All lengths are SI metres; samples sit at voxel centres
//! `centre + (i − (n−1)/2)·h` per axis, stored X-outermost row-major (the
//! layout `densify_grid_to_sampled` reads back). Resolution, padding and the
//! activation policy are `docs/prds/v0_6/implicit-solids.md` D-7 / D-8.

use std::fmt;

use reify_core::ContentHash;

/// Options for ingesting a [`DenseSdfGrid`] as a level-set solid.
///
/// `band_voxels` is both the per-side padding of the grid and the ±clamp of
/// the stored distances (D-8); see [`Self::band_half_width`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImplicitOptions {
    voxel_size: f64,
    band_voxels: u32,
}

impl ImplicitOptions {
    pub fn new(voxel_size: f64, band_voxels: u32) -> Result<Self, ImplicitGridError> {
        if !is_valid_voxel_size(voxel_size) {
            return Err(ImplicitGridError::InvalidVoxelSize { value: voxel_size });
        }
        if band_voxels == 0 {
            return Err(ImplicitGridError::ZeroBand);
        }
        Ok(Self {
            voxel_size,
            band_voxels,
        })
    }

    pub fn voxel_size(&self) -> f64 {
        self.voxel_size
    }

    pub fn band_voxels(&self) -> u32 {
        self.band_voxels
    }

    /// The narrow-band half width `band_voxels · voxel_size`: the ± clamp on
    /// every stored distance and the level set's background value.
    pub fn band_half_width(&self) -> f64 {
        f64::from(self.band_voxels) * self.voxel_size
    }

    /// Cache key for these options. Wire order is fixed: domain tag →
    /// `voxel_size` LE bytes → `band_voxels` LE bytes. The domain tag keeps the
    /// hash away from the `ContentHash(0)` NO_OPTIONS sentinel.
    pub fn content_hash(&self) -> ContentHash {
        ContentHash::of_str("ImplicitOptions")
            .combine(ContentHash::of(&self.voxel_size.to_le_bytes()))
            .combine(ContentHash::of(&self.band_voxels.to_le_bytes()))
    }
}

/// A dense, axis-aligned block of f32 signed-distance samples.
#[derive(Clone, PartialEq)]
pub struct DenseSdfGrid {
    dims: [u32; 3],
    centre: [f64; 3],
    h: f64,
    data: Vec<f32>,
}

impl DenseSdfGrid {
    /// Samples `f` at every voxel centre. The shape, spacing and centre are
    /// validated before anything is allocated or sampled, and a sample that is
    /// not finite once stored as f32 is refused, naming its voxel. The fill is
    /// sequential, so the result is deterministic for a deterministic `f`.
    ///
    /// The voxel count is checked only for `usize` overflow, not against a
    /// memory budget: sizing the grid within the D-7 budget is the caller's
    /// job, and the kernel's over-budget refusal at ingest comes after this
    /// buffer has been allocated and filled.
    pub fn from_fn(
        dims: [u32; 3],
        centre: [f64; 3],
        h: f64,
        mut f: impl FnMut([f64; 3]) -> f64,
    ) -> Result<Self, ImplicitGridError> {
        if let Some(axis) = dims.iter().position(|&n| n < MIN_VOXELS_PER_AXIS) {
            return Err(ImplicitGridError::DegenerateAxis { axis, dims });
        }
        if !is_valid_voxel_size(h) {
            return Err(ImplicitGridError::InvalidVoxelSize { value: h });
        }
        if !centre.iter().all(|c| c.is_finite()) {
            return Err(ImplicitGridError::NonFiniteCentre { centre });
        }
        let count = dims
            .iter()
            .try_fold(1usize, |acc, &n| acc.checked_mul(n as usize))
            .ok_or(ImplicitGridError::VoxelCountOverflow { dims })?;

        let mut data = Vec::with_capacity(count);
        for ix in 0..dims[0] {
            for iy in 0..dims[1] {
                for iz in 0..dims[2] {
                    let index = [ix, iy, iz];
                    let value = f(voxel_centre_of(dims, centre, h, index));
                    let stored = value as f32;
                    if !stored.is_finite() {
                        return Err(ImplicitGridError::NonFiniteSample { index, value });
                    }
                    data.push(stored);
                }
            }
        }
        Ok(Self {
            dims,
            centre,
            h,
            data,
        })
    }

    pub fn dims(&self) -> [u32; 3] {
        self.dims
    }

    /// World position of the block's centre, metres.
    pub fn centre(&self) -> [f64; 3] {
        self.centre
    }

    /// Voxel side length, metres.
    pub fn h(&self) -> f64 {
        self.h
    }

    /// Samples, X-outermost row-major: `data[(ix·ny + iy)·nz + iz]`.
    pub fn data(&self) -> &[f32] {
        &self.data
    }

    /// World position of the centre of voxel `index`, metres.
    pub fn voxel_centre(&self, index: [u32; 3]) -> [f64; 3] {
        voxel_centre_of(self.dims, self.centre, self.h, index)
    }

    /// Refuses options whose voxel size differs from the grid's spacing. The
    /// comparison is exact: both are built from one `h`.
    pub fn check_options(&self, opts: &ImplicitOptions) -> Result<(), ImplicitGridError> {
        if self.h == opts.voxel_size() {
            Ok(())
        } else {
            Err(ImplicitGridError::VoxelSizeMismatch {
                grid_h: self.h,
                options_voxel_size: opts.voxel_size(),
            })
        }
    }
}

impl fmt::Debug for DenseSdfGrid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DenseSdfGrid")
            .field("dims", &self.dims)
            .field("centre", &self.centre)
            .field("h", &self.h)
            .field("data", &format_args!("<{} f32 samples>", self.data.len()))
            .finish()
    }
}

/// `densify_grid_to_sampled` reads a grid back through a sampled field that
/// needs two nodes per axis, so a thinner grid could be ingested but not read.
const MIN_VOXELS_PER_AXIS: u32 = 2;

fn is_valid_voxel_size(h: f64) -> bool {
    h.is_finite() && h > 0.0
}

fn voxel_centre_of(dims: [u32; 3], centre: [f64; 3], h: f64, index: [u32; 3]) -> [f64; 3] {
    std::array::from_fn(|a| centre[a] + (f64::from(index[a]) - f64::from(dims[a] - 1) / 2.0) * h)
}

/// A violated [`DenseSdfGrid`] / [`ImplicitOptions`] invariant, carrying the
/// offending values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ImplicitGridError {
    InvalidVoxelSize {
        value: f64,
    },
    ZeroBand,
    DegenerateAxis {
        axis: usize,
        dims: [u32; 3],
    },
    VoxelCountOverflow {
        dims: [u32; 3],
    },
    NonFiniteCentre {
        centre: [f64; 3],
    },
    NonFiniteSample {
        index: [u32; 3],
        value: f64,
    },
    VoxelSizeMismatch {
        grid_h: f64,
        options_voxel_size: f64,
    },
}

impl fmt::Display for ImplicitGridError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidVoxelSize { value } => {
                write!(f, "voxel size must be finite and > 0 m; got {value}")
            }
            Self::ZeroBand => write!(f, "band_voxels must be at least 1; got 0"),
            Self::DegenerateAxis { axis, dims } => write!(
                f,
                "every grid axis needs at least {MIN_VOXELS_PER_AXIS} voxels; axis {axis} of \
                 dims {dims:?} has {}",
                dims[*axis]
            ),
            Self::VoxelCountOverflow { dims } => {
                write!(f, "voxel count of dims {dims:?} overflows usize")
            }
            Self::NonFiniteCentre { centre } => {
                write!(f, "grid centre must be finite; got {centre:?}")
            }
            Self::NonFiniteSample { index, value } => write!(
                f,
                "every SDF sample must be finite as f32; voxel {index:?} sampled {value}"
            ),
            Self::VoxelSizeMismatch {
                grid_h,
                options_voxel_size,
            } => write!(
                f,
                "grid voxel size {grid_h} m differs from ImplicitOptions voxel size \
                 {options_voxel_size} m"
            ),
        }
    }
}

impl std::error::Error for ImplicitGridError {}

#[cfg(test)]
mod tests {
    use super::{DenseSdfGrid, ImplicitGridError, ImplicitOptions};
    use reify_core::ContentHash;

    // ── ImplicitOptions::new ────────────────────────────────────────────────

    #[test]
    fn implicit_options_refuse_non_positive_or_non_finite_voxel_size() {
        for bad in [0.0, -1e-3, f64::NAN, f64::INFINITY] {
            match ImplicitOptions::new(bad, 3) {
                Err(ImplicitGridError::InvalidVoxelSize { value }) => {
                    assert!(
                        value.to_bits() == bad.to_bits(),
                        "InvalidVoxelSize must carry the offending value {bad:?}; got {value:?}",
                    );
                }
                other => panic!("voxel_size {bad:?} must be refused; got {other:?}"),
            }
        }
    }

    #[test]
    fn implicit_options_refuse_zero_band() {
        assert_eq!(
            ImplicitOptions::new(5e-4, 0),
            Err(ImplicitGridError::ZeroBand)
        );
    }

    #[test]
    fn implicit_options_accessors_and_band_half_width() {
        let opts = ImplicitOptions::new(5e-4, 3).expect("valid options");
        assert_eq!(opts.voxel_size(), 5e-4);
        assert_eq!(opts.band_voxels(), 3);
        assert_eq!(opts.band_half_width(), 3.0 * 5e-4);
    }

    // ── ImplicitOptions::content_hash ───────────────────────────────────────

    #[test]
    fn implicit_options_content_hash_is_not_no_options_sentinel() {
        let hash = ImplicitOptions::new(5e-4, 3).unwrap().content_hash();
        assert_ne!(
            hash,
            ContentHash(0),
            "ImplicitOptions::content_hash must not equal ContentHash(0), the NO_OPTIONS \
             sentinel of reify_eval::realization_cache (reify-ir cannot import reify-eval); \
             the non-zero domain tag of_str(\"ImplicitOptions\") must seal this",
        );
    }

    #[test]
    fn implicit_options_content_hash_is_sensitive_to_voxel_size() {
        let a = ImplicitOptions::new(5e-4, 3).unwrap();
        let b = ImplicitOptions::new(2.5e-4, 3).unwrap();
        assert_ne!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn implicit_options_content_hash_is_sensitive_to_band_voxels() {
        let a = ImplicitOptions::new(5e-4, 3).unwrap();
        let b = ImplicitOptions::new(5e-4, 4).unwrap();
        assert_ne!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn implicit_options_content_hash_is_deterministic() {
        let a = ImplicitOptions::new(5e-4, 3).unwrap();
        let b = ImplicitOptions::new(5e-4, 3).unwrap();
        assert_eq!(a.content_hash(), b.content_hash());
    }

    // ── DenseSdfGrid::from_fn refusals (all before any sample is taken) ─────

    /// Builds a grid with a closure that counts its calls; returns the result
    /// and the call count, so a refusal can be shown to precede the fill.
    fn from_fn_counting(
        dims: [u32; 3],
        centre: [f64; 3],
        h: f64,
    ) -> (Result<DenseSdfGrid, ImplicitGridError>, usize) {
        let mut calls = 0usize;
        let result = DenseSdfGrid::from_fn(dims, centre, h, |_| {
            calls += 1;
            0.0
        });
        (result, calls)
    }

    /// A single-voxel axis could be ingested but never densified back
    /// (`densify_grid_to_sampled` needs two nodes per axis), so it is refused
    /// alongside an empty one.
    #[test]
    fn from_fn_refuses_an_axis_under_two_voxels_before_sampling() {
        for dims in [[3, 0, 2], [3, 1, 2]] {
            let (result, calls) = from_fn_counting(dims, [0.0; 3], 1e-3);
            assert_eq!(
                result,
                Err(ImplicitGridError::DegenerateAxis { axis: 1, dims }),
                "dims {dims:?}",
            );
            assert_eq!(calls, 0, "refusal must precede the fill");
        }
    }

    #[test]
    fn from_fn_accepts_two_voxels_per_axis() {
        let (result, calls) = from_fn_counting([2, 2, 2], [0.0; 3], 1e-3);
        assert!(result.is_ok(), "got {result:?}");
        assert_eq!(calls, 8);
    }

    #[test]
    fn from_fn_refuses_non_positive_or_non_finite_h_before_sampling() {
        for bad in [0.0, -1e-3, f64::NAN] {
            let (result, calls) = from_fn_counting([2, 2, 2], [0.0; 3], bad);
            assert!(
                matches!(result, Err(ImplicitGridError::InvalidVoxelSize { value })
                    if value.to_bits() == bad.to_bits()),
                "h {bad:?} must be refused; got {result:?}",
            );
            assert_eq!(calls, 0, "refusal must precede the fill");
        }
    }

    #[test]
    fn from_fn_refuses_a_non_finite_centre_before_sampling() {
        let (result, calls) = from_fn_counting([2, 2, 2], [0.0, f64::NAN, 0.0], 1e-3);
        assert!(
            matches!(result, Err(ImplicitGridError::NonFiniteCentre { centre })
                if centre[0] == 0.0 && centre[1].is_nan() && centre[2] == 0.0),
            "got {result:?}",
        );
        assert_eq!(calls, 0, "refusal must precede the fill");
    }

    #[test]
    fn from_fn_refuses_a_non_finite_sample_naming_its_voxel() {
        let dims = [3, 4, 5];
        let h = 1e-3;
        let probe = DenseSdfGrid::from_fn(dims, [0.0; 3], h, |_| 0.0).unwrap();
        let bad_index = [2, 1, 3];
        let bad_point = probe.voxel_centre(bad_index);
        // 1e300 is finite as f64 but overflows to +inf as f32.
        for bad in [f64::NAN, f64::INFINITY, 1e300] {
            let result =
                DenseSdfGrid::from_fn(
                    dims,
                    [0.0; 3],
                    h,
                    |p| {
                        if p == bad_point { bad } else { 0.0 }
                    },
                );
            assert!(
                matches!(result, Err(ImplicitGridError::NonFiniteSample { index, value })
                    if index == bad_index && value.to_bits() == bad.to_bits()),
                "sample {bad:?} at {bad_index:?} must be refused; got {result:?}",
            );
        }
    }

    // ── layout contract ─────────────────────────────────────────────────────

    #[test]
    fn from_fn_fills_x_outermost_row_major_at_voxel_centres() {
        let dims = [3u32, 4, 5];
        let centre = [0.01, -0.02, 0.005];
        let h = 1e-3;
        let f = |p: [f64; 3]| p[0] * 1e3 + p[1] * 1e1 + p[2];
        let grid = DenseSdfGrid::from_fn(dims, centre, h, f).expect("valid grid");

        assert_eq!(grid.dims(), dims);
        assert_eq!(grid.centre(), centre);
        assert_eq!(grid.h(), h);
        assert_eq!(grid.data().len(), 60);

        for ix in 0..dims[0] {
            for iy in 0..dims[1] {
                for iz in 0..dims[2] {
                    let linear = (ix * 4 * 5 + iy * 5 + iz) as usize;
                    let expected = f(grid.voxel_centre([ix, iy, iz])) as f32;
                    assert_eq!(
                        grid.data()[linear].to_bits(),
                        expected.to_bits(),
                        "sample at [{ix},{iy},{iz}] (linear {linear})",
                    );
                }
            }
        }

        let first = grid.voxel_centre([0, 0, 0]);
        let last = grid.voxel_centre([dims[0] - 1, dims[1] - 1, dims[2] - 1]);
        for a in 0..3 {
            let half_span = (dims[a] - 1) as f64 * h / 2.0;
            assert!(
                (first[a] - (centre[a] - half_span)).abs() <= 1e-15,
                "axis {a}: first voxel centre {} != centre − (n−1)·h/2 = {}",
                first[a],
                centre[a] - half_span,
            );
            assert!(
                (last[a] - (centre[a] + half_span)).abs() <= 1e-15,
                "axis {a}: last voxel centre {} != centre + (n−1)·h/2 = {}",
                last[a],
                centre[a] + half_span,
            );
        }
    }

    // ── check_options ───────────────────────────────────────────────────────

    #[test]
    fn check_options_accepts_a_matching_voxel_size() {
        let grid = DenseSdfGrid::from_fn([2, 2, 2], [0.0; 3], 5e-4, |_| 0.0).unwrap();
        let opts = ImplicitOptions::new(5e-4, 3).unwrap();
        assert_eq!(grid.check_options(&opts), Ok(()));
    }

    #[test]
    fn check_options_refuses_a_mismatched_voxel_size_naming_both() {
        let grid = DenseSdfGrid::from_fn([2, 2, 2], [0.0; 3], 5e-4, |_| 0.0).unwrap();
        let opts = ImplicitOptions::new(1e-3, 3).unwrap();
        let err = grid.check_options(&opts).unwrap_err();
        assert_eq!(
            err,
            ImplicitGridError::VoxelSizeMismatch {
                grid_h: 5e-4,
                options_voxel_size: 1e-3,
            }
        );
        let msg = err.to_string();
        assert!(
            msg.contains(&5e-4f64.to_string()) && msg.contains(&1e-3f64.to_string()),
            "Display must name both voxel sizes; got {msg:?}",
        );
    }

    // ── Debug ───────────────────────────────────────────────────────────────

    #[test]
    fn debug_elides_the_samples() {
        let grid = DenseSdfGrid::from_fn([32, 32, 32], [0.0; 3], 1e-3, |p| p[0]).unwrap();
        let rendered = format!("{grid:?}");
        assert!(
            rendered.len() < 300,
            "Debug must not dump the samples; got {} chars",
            rendered.len(),
        );
        assert!(
            rendered.contains("32768"),
            "Debug must report the sample count; got {rendered:?}",
        );
    }
}
