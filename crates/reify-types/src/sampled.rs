//! Shared axis-grid utilities for sampled fields.
//!
//! Both `reify-eval` (user-defined `sampled` field sources) and
//! `reify-kernel-openvdb` (imported OpenVDB grids) need the same
//! axis-grid builder.  This module is the single canonical home so
//! that the two call-sites share one implementation and one cap constant.

/// Maximum number of intervals allowed per axis in [`linspace_inclusive`].
///
/// A legitimately-finite but enormous combination such as
/// `bounds_min = 0.0`, `bounds_max = 1e308`, `spacing = 1.0` would
/// make `(span / spacing).round() as usize` saturate on overflow and
/// attempt to allocate an astronomically large `Vec`.  This constant
/// provides a hard upper bound that rejects such inputs before any
/// allocation occurs.
///
/// 10 million intervals ≈ 80 MB for `Vec<f64>` per axis, which is large
/// but still physically meaningful for scientific datasets (e.g. a 1 mm
/// resolution 10 km grid).  Callers that exceed this limit should treat
/// it as a configuration error and surface a user-facing diagnostic.
pub const LINSPACE_MAX_INTERVALS: usize = 10_000_000;

/// Inclusive linspace from `start` to `stop` with step `spacing`.
///
/// Produces `[start, start+spacing, …, stop]` (or as close as
/// `round((stop-start)/spacing)` admits).  Returns `Some([start])` for
/// degenerate-but-valid inputs (non-positive spacing or `stop < start`).
///
/// Returns `None` when the computed interval count exceeds
/// [`LINSPACE_MAX_INTERVALS`], indicating a configuration error that
/// the caller should surface as a user-facing diagnostic rather than
/// attempting the allocation.
///
/// # Why this lives in `reify-types`
///
/// This is the single canonical implementation for the two downstream
/// sampled-field call sites (`reify-eval::build_sampled_field` and
/// `reify-kernel-openvdb::lower_to_sampled`).  Each call site maps `None`
/// to its own domain error convention without this crate needing to own a
/// linspace-specific error enum.
///
/// # Defensive properties
///
/// The `spacing <= 0.0 || !is_finite()` and `span < 0.0` branches are
/// defense-in-depth.  Both downstream call sites pre-flight-check
/// finite/positive spacing before calling this helper.  They remain here
/// so this function stays safe to call from any future site that might not
/// run the same pre-flight checks.
pub fn linspace_inclusive(start: f64, stop: f64, spacing: f64) -> Option<Vec<f64>> {
    // Defense-in-depth: callers pre-flight-check these, but we guard here too.
    if spacing <= 0.0 || !spacing.is_finite() || !start.is_finite() || !stop.is_finite() {
        return Some(vec![start]);
    }
    let span = stop - start;
    if span < 0.0 {
        return Some(vec![start]);
    }
    // Round to nearest integer to avoid floating-point cliff effects:
    // e.g. (2.0 - 0.0) / 1.0 may evaluate to 1.999… which .floor() → 1,
    // producing [0.0, 1.0] instead of [0.0, 1.0, 2.0].
    let n_intervals = (span / spacing).round() as usize;
    if n_intervals > LINSPACE_MAX_INTERVALS {
        return None;
    }
    Some((0..=n_intervals).map(|i| start + (i as f64) * spacing).collect())
}

#[cfg(test)]
mod tests {
    use super::{LINSPACE_MAX_INTERVALS, linspace_inclusive};

    #[test]
    fn basic_linspace() {
        let v = linspace_inclusive(0.0, 2.0, 1.0).expect("should not be capped");
        assert_eq!(v, vec![0.0, 1.0, 2.0]);
    }

    #[test]
    fn degenerate_negative_span() {
        // stop < start → degenerate; returns [start]
        let v = linspace_inclusive(1.0, 0.0, 1.0).expect("degenerate is Some");
        assert_eq!(v, vec![1.0]);
    }

    #[test]
    fn degenerate_non_positive_spacing() {
        // spacing <= 0 → degenerate; returns [start]
        let v = linspace_inclusive(0.0, 1.0, -1.0).expect("degenerate is Some");
        assert_eq!(v, vec![0.0]);
    }

    #[test]
    fn cap_returns_none() {
        // bounds_min=0.0, bounds_max=1e308, spacing=1.0 → far exceeds cap
        let result = linspace_inclusive(0.0, 1e308, 1.0);
        assert!(result.is_none(), "expected None for cap-exceeding input");
    }

    #[test]
    fn cap_boundary_just_under() {
        // start=0.0, stop=LINSPACE_MAX_INTERVALS as f64, spacing=1.0
        // → n_intervals == LINSPACE_MAX_INTERVALS exactly → should be Some
        let stop = LINSPACE_MAX_INTERVALS as f64;
        let v = linspace_inclusive(0.0, stop, 1.0).expect("exactly at cap should be Some");
        assert_eq!(v.len(), LINSPACE_MAX_INTERVALS + 1);
    }
}
