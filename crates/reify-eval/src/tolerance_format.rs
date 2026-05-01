//! Human-readable unit-prefix formatter for tolerance diagnostic messages.
//!
//! Shared across the four `tolerance_*` modules so all diagnostic messages
//! use the same `µm / mm / m` magnitude bands.
//!
//! # Band breakpoints
//!
//! | Condition            | Unit | Example output |
//! |----------------------|------|----------------|
//! | `si < 1e-3` m        | µm   | `50µm`         |
//! | `1e-3 ≤ si < 1.0` m  | mm   | `5mm`          |
//! | `si ≥ 1.0` m         | m    | `1.5m`         |
//! | `si == 0.0`          | m    | `0m`           |
//! | non-finite / negative | m   | raw fallback   |

/// Format an SI-metres tolerance value as a human-readable string.
///
/// Uses the µm / mm / m magnitude bands that are already established in
/// `tolerance_promise`'s truth-table docstring and across the test suite.
/// Non-finite or negative inputs fall through to a raw `"{x}m"` fallback
/// so the helper never panics even if upstream invariants are broken.
pub(crate) fn format_tolerance(si_metres: f64) -> String {
    if !si_metres.is_finite() || si_metres < 0.0 {
        return format!("{si_metres}m");
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
}
