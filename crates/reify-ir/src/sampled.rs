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
///
/// # `cfg(test)` note
///
/// Within `reify-types`'s own unit tests, this constant is set to `8`
/// to keep cap-boundary tests fast (< 1 µs instead of ~500 ms for
/// the 10 M allocation).  Downstream crates (`reify-eval`,
/// `reify-kernel-openvdb`) compile `reify-types` without `cfg(test)`,
/// so they always see the production cap (`10_000_000`).
#[cfg(not(test))]
pub const LINSPACE_MAX_INTERVALS: usize = 10_000_000;

/// # ⚠ WARNING — test-only shrunk cap
///
/// This `cfg(test)` override sets `LINSPACE_MAX_INTERVALS` to `8` **only**
/// inside `reify-types`'s own unit-test binary.  The value is intentionally
/// small to keep the cap-boundary tests fast (< 1 µs instead of ~500 ms for
/// the 10 M allocation).
///
/// The `cap_boundary_*` tests in `mod tests` at the bottom of this file are
/// the legitimate consumers — they treat `LINSPACE_MAX_INTERVALS` as a
/// **symbolic boundary value** and never compare it to the literal
/// `10_000_000` or depend on its magnitude.
///
/// **If you add a new `reify-types` test that references `LINSPACE_MAX_INTERVALS`
/// by name, you MUST follow one of these patterns:**
///
/// - **Hardcode** the expected value your test cares about (e.g. write
///   `assert_eq!(result, 10_000_000)` rather than
///   `assert_eq!(result, LINSPACE_MAX_INTERVALS)`), or
/// - **Gate** on the production cap explicitly (e.g. declare a local
///   `const PROD_CAP: usize = 10_000_000;` with an explanatory comment).
///
/// Do **not** compare `LINSPACE_MAX_INTERVALS` against the literal
/// `10_000_000` inside a `#[cfg(test)]` context or otherwise rely on the
/// production magnitude — the constant silently resolves to `8` there.
///
/// Downstream crates (`reify-eval`, `reify-kernel-openvdb`) compile
/// `reify-types` without `cfg(test)`, so they always see the production
/// cap (`10_000_000`).
#[cfg(test)]
pub const LINSPACE_MAX_INTERVALS: usize = 8;

/// Reason why [`linspace_inclusive`] rejected its input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinspaceError {
    /// The computed interval count exceeds [`LINSPACE_MAX_INTERVALS`].
    ///
    /// Carries the **finite** computed count so callers can embed it in a
    /// user-facing diagnostic (e.g. "requires 11 000 000 grid intervals,
    /// exceeds the 10 000 000 interval cap") without recomputing it.
    ///
    /// This variant is only returned when the span/spacing ratio is strictly
    /// less than `usize::MAX as f64` (= 2^64 on 64-bit targets), so the
    /// cast to `usize` is exact and the count fits in a `usize`.
    Excessive {
        /// The computed interval count.  Guaranteed to be
        /// `> LINSPACE_MAX_INTERVALS` and representable as a finite `usize`.
        n_intervals: usize,
    },
    /// `(stop - start) / spacing` exceeds `usize::MAX as f64`.
    ///
    /// The interval count cannot be meaningfully represented as a `usize`,
    /// so no numeric payload is carried — embedding a saturated `usize::MAX`
    /// in a user-facing diagnostic would falsely imply a precise (though
    /// absurd) count.  Distinct from [`LinspaceError::Excessive`], which
    /// always carries a valid finite count.
    Overflow,
}

/// Inclusive linspace from `start` to `stop` with step `spacing`.
///
/// Produces `[start, start+spacing, …, stop]` (or as close as
/// `round((stop-start)/spacing)` admits).  Returns `Ok([start])` for
/// degenerate-but-valid inputs (non-positive spacing or `stop < start`).
///
/// # Errors
///
/// - [`LinspaceError::Overflow`] — when `(stop - start) / spacing` exceeds
///   `usize::MAX as f64`.  The count is not representable; callers should
///   emit a distinct "overflow" diagnostic rather than a cap-exceeded one.
/// - [`LinspaceError::Excessive`] — when the computed interval count exceeds
///   [`LINSPACE_MAX_INTERVALS`] but still fits in a `usize`.  The finite
///   count is embedded in the error for use in user-facing diagnostics.
///
/// # Why this lives in `reify-types`
///
/// This is the single canonical implementation for the two downstream
/// sampled-field call sites (`reify-eval::build_sampled_field` and
/// `reify-kernel-openvdb::lower_to_sampled`).  Each call site maps
/// `LinspaceError` to its own domain error convention.
///
/// # Defensive properties
///
/// The `spacing <= 0.0 || !is_finite()` and `span < 0.0` branches are
/// defense-in-depth.  Both downstream call sites pre-flight-check
/// finite/positive spacing before calling this helper.  They remain here
/// so this function stays safe to call from any future site that might not
/// run the same pre-flight checks.
pub fn linspace_inclusive(start: f64, stop: f64, spacing: f64) -> Result<Vec<f64>, LinspaceError> {
    // Defense-in-depth: callers pre-flight-check these, but we guard here too.
    if spacing <= 0.0 || !spacing.is_finite() || !start.is_finite() || !stop.is_finite() {
        return Ok(vec![start]);
    }
    let span = stop - start;
    if span < 0.0 {
        return Ok(vec![start]);
    }
    // Detect overflow BEFORE the `as usize` cast.
    //
    // On 64-bit platforms, `usize::MAX as f64` rounds UP to 2^64 (the nearest
    // representable f64 ≥ usize::MAX, since f64 has only a 53-bit mantissa and
    // 2^64-1 is not exactly representable).  Using `>=` therefore catches both:
    //   • ratio == 2^64 exactly — `2^64 as usize` would saturate to usize::MAX
    //   • ratio > 2^64 — plainly overflows
    // Values below 2^64 produce valid, finite usize values via `as usize`; the
    // largest representable f64 below 2^64 is 2^64 - 2048, which still greatly
    // exceeds the production cap (10 M) and is caught by the Excessive branch.
    let ratio = span / spacing;
    if ratio >= usize::MAX as f64 {
        return Err(LinspaceError::Overflow);
    }
    // Round to nearest integer to avoid floating-point cliff effects:
    // e.g. (2.0 - 0.0) / 1.0 may evaluate to 1.999… which .floor() → 1,
    // producing [0.0, 1.0] instead of [0.0, 1.0, 2.0].
    let n_intervals = ratio.round() as usize;
    if n_intervals > LINSPACE_MAX_INTERVALS {
        return Err(LinspaceError::Excessive { n_intervals });
    }
    Ok((0..=n_intervals)
        .map(|i| start + (i as f64) * spacing)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{LINSPACE_MAX_INTERVALS, LinspaceError, linspace_inclusive};

    #[test]
    fn basic_linspace() {
        let v = linspace_inclusive(0.0, 2.0, 1.0).expect("should not be capped");
        assert_eq!(v, vec![0.0, 1.0, 2.0]);
    }

    #[test]
    fn degenerate_negative_span() {
        // stop < start → degenerate; returns [start]
        let v = linspace_inclusive(1.0, 0.0, 1.0).expect("degenerate is Ok");
        assert_eq!(v, vec![1.0]);
    }

    #[test]
    fn degenerate_non_positive_spacing() {
        // spacing <= 0 → degenerate; returns [start]
        let v = linspace_inclusive(0.0, 1.0, -1.0).expect("degenerate is Ok");
        assert_eq!(v, vec![0.0]);
    }

    #[test]
    fn cap_overflow_returns_overflow_variant() {
        // bounds_min=0.0, bounds_max=1e308, spacing=1.0 → ratio overflows usize
        assert!(matches!(
            linspace_inclusive(0.0, 1e308, 1.0),
            Err(LinspaceError::Overflow)
        ));
    }

    #[test]
    fn cap_boundary_just_under() {
        // start=0.0, stop=LINSPACE_MAX_INTERVALS as f64, spacing=1.0
        // → n_intervals == LINSPACE_MAX_INTERVALS exactly → should be Ok.
        // With cfg(test) cap = 8: allocates 9 f64 (< 100 bytes) instead of ~80 MB.
        let stop = LINSPACE_MAX_INTERVALS as f64;
        let v = linspace_inclusive(0.0, stop, 1.0).expect("exactly at cap should be Ok");
        assert_eq!(v.len(), LINSPACE_MAX_INTERVALS + 1);
    }

    #[test]
    fn cap_boundary_just_over() {
        // n_intervals == LINSPACE_MAX_INTERVALS + 1 → Err(Excessive).
        // Guards against an off-by-one flip from `>` to `>=` in the cap check.
        let result = linspace_inclusive(0.0, (LINSPACE_MAX_INTERVALS + 1) as f64, 1.0);
        assert!(
            matches!(result, Err(LinspaceError::Excessive { n_intervals }) if n_intervals == LINSPACE_MAX_INTERVALS + 1)
        );
    }

    #[test]
    fn cap_excessive_n_intervals_is_finite_when_just_over_cap() {
        // Pins that the Err variant carries the EXACT finite count rather than
        // a saturated sentinel (usize::MAX).  Parity with OpenVDB ingest, which
        // embeds n_intervals in the IngestError::ExcessiveAxisLength payload.
        let expected = LINSPACE_MAX_INTERVALS + 1;
        match linspace_inclusive(0.0, expected as f64, 1.0) {
            Err(LinspaceError::Excessive { n_intervals }) => {
                assert_eq!(
                    n_intervals, expected,
                    "n_intervals should be the exact finite count {expected}, not a sentinel"
                );
            }
            other => panic!(
                "expected Err(LinspaceError::Excessive {{ n_intervals: {expected} }}), got {other:?}"
            ),
        }
    }
}
