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

    #[test]
    fn from_fn_refuses_an_empty_axis_before_sampling() {
        let (result, calls) = from_fn_counting([3, 0, 2], [0.0; 3], 1e-3);
        assert!(
            matches!(
                result,
                Err(ImplicitGridError::EmptyAxis { axis: 1, dims: [3, 0, 2] })
            ),
            "got {result:?}",
        );
        assert_eq!(calls, 0, "refusal must precede the fill");
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
            let result = DenseSdfGrid::from_fn(dims, [0.0; 3], h, |p| {
                if p == bad_point { bad } else { 0.0 }
            });
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
