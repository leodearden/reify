//! Human-readable unit-prefix formatter for tolerance diagnostic messages.
//!
//! Shared across the four `tolerance_*` modules so all diagnostic messages
//! use the same `µm / mm / m` magnitude bands.

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
