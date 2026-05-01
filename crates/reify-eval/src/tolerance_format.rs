//! Human-readable unit-prefix formatter for tolerance diagnostic messages.
//!
//! Intended to be shared across the four `tolerance_*` modules for consistent
//! `µm / mm / m` magnitude bands in diagnostic messages.
//!
//! Currently only `tolerance_promise` calls this helper.
//! TODO(task-2790-follow-up): migrate `tolerance_combine`, `tolerance_bucket`,
//! and `tolerance_budget` raw f64-metres format sites to use `format_tolerance`.
//!
//! # Band breakpoints
//!
//! | Condition             | Unit | Example output |
//! |-----------------------|------|----------------|
//! | `si < 1e-3` m         | µm   | `50µm`         |
//! | `1e-3 ≤ si < 1.0` m   | mm   | `5mm`          |
//! | `si ≥ 1.0` m          | m    | `1.5m`         |
//! | `si == 0.0`           | m    | `0m`           |
//! | non-finite / negative | —    | `"{x} m"` (release fallback; debug-asserts) |

/// Format an SI-metres tolerance value as a human-readable string.
///
/// Uses the µm / mm / m magnitude bands that are already established in
/// `tolerance_promise`'s truth-table docstring and across the test suite.
///
/// # Panics
///
/// In debug builds, panics via `debug_assert!` if `si_metres` is non-finite
/// (NaN or ±∞) or negative — the upstream `promise > demanded > 0` invariant
/// (enforced by `is_promise_insufficient` and the silent-skip extractors) makes
/// these inputs unreachable in practice, so the assert is a development
/// tripwire, not a runtime check.
///
/// In release builds, the same inputs fall through to a `"{x} m"` fallback
/// (space-separated to avoid ambiguous strings like `"NaNm"` or `"infm"`)
/// so the helper never panics in production even if upstream invariants
/// are broken.
pub(crate) fn format_tolerance(si_metres: f64) -> String {
    debug_assert!(
        si_metres.is_finite() && si_metres >= 0.0,
        "format_tolerance invariant violated: si_metres={si_metres}",
    );
    if !si_metres.is_finite() || si_metres < 0.0 {
        return format!("{si_metres} m");
    }
    if si_metres == 0.0 {
        return "0m".to_string();
    }
    if si_metres < 1e-3 {
        format!("{}µm", si_metres * 1e6)
    } else if si_metres < 1.0 {
        format!("{}mm", si_metres * 1e3)
    } else {
        format!("{si_metres}m")
    }
}

#[cfg(test)]
mod tests {
    use super::format_tolerance;

    #[test]
    fn format_tolerance_microns() {
        assert_eq!(format_tolerance(50e-6), "50µm");
    }

    #[test]
    fn format_tolerance_millimetres() {
        assert_eq!(format_tolerance(5e-3), "5mm");
    }

    #[test]
    fn format_tolerance_metres() {
        assert_eq!(format_tolerance(1.0), "1m");
    }

    #[test]
    fn format_tolerance_zero() {
        assert_eq!(format_tolerance(0.0), "0m");
    }

    #[test]
    fn format_tolerance_boundary_mm() {
        // 1e-3 is the lower bound of the mm band (inclusive)
        assert_eq!(format_tolerance(1e-3), "1mm");
    }

    #[test]
    fn format_tolerance_sub_micron() {
        // sub-µm sanity: 1e-9 m = 0.001 µm
        assert_eq!(format_tolerance(1e-9), "0.001µm");
    }

    #[test]
    fn format_tolerance_near_mm_boundary_stays_microns() {
        // 0.999_999e-3 m is just below the 1e-3 band boundary, so it renders
        // in µm. Due to f64 arithmetic (`0.999_999e-3 * 1e6` is not exactly
        // 999.999), the shortest-round-trip Display produces a long decimal:
        // "999.9989999999999µm". This test pins that actual output so the
        // formatting choice (no precision spec, raw `{}`) is explicit rather
        // than assumed to be exact.
        assert_eq!(format_tolerance(0.999_999e-3), "999.9989999999999µm");
    }

    #[test]
    fn format_tolerance_metres_non_integer() {
        // A value in the m-band that isn't an integer — pins that no
        // precision spec is applied and the output matches f64 Display.
        assert_eq!(format_tolerance(1.5), "1.5m");
    }

    // Debug-build NaN/+Inf/negative panic tests. Mirror the
    // `is_promise_insufficient_panics_in_debug_on_*` precedent in
    // `tolerance_promise.rs:487-540`. The `expected` string is the static
    // prefix of the canonical message — substring matching avoids depending
    // on the dynamic interpolated `si_metres` value.

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "format_tolerance invariant violated")]
    fn format_tolerance_panics_in_debug_on_negative() {
        format_tolerance(-1e-5);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "format_tolerance invariant violated")]
    fn format_tolerance_panics_in_debug_on_nan() {
        format_tolerance(f64::NAN);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "format_tolerance invariant violated")]
    fn format_tolerance_panics_in_debug_on_positive_infinity() {
        format_tolerance(f64::INFINITY);
    }

    // Release-mode regression guard: pins that the fallback for non-finite /
    // negative inputs is space-separated (`"-0.00001 m"`, not `"-0.00001m"`).
    // This test is excluded from default `cargo test` (debug mode) but runs
    // under `cargo test --release`, exercising the production code path.
    #[cfg(not(debug_assertions))]
    #[test]
    fn format_tolerance_negative_release_fallback_is_space_separated() {
        assert_eq!(format_tolerance(-1e-5), "-0.00001 m");
    }
}
