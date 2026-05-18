//! Integration tests for the Featherstone 6D spatial-vector core
//! (`reify_stdlib::dynamics::spatial`).
//!
//! Mirrors the `tests/complex_tests.rs` layout: top-of-file `use`, per-behavior
//! `mod` blocks, shared tolerance/entrywise-equality helpers at the top.
//!
//! Convention (Featherstone 2008, §2.4): spatial vectors are ordered
//! `[ω_x, ω_y, ω_z, v_x, v_y, v_z]` — angular first, linear second. 6×6
//! matrices are row-major `[f64; 36]`.

use reify_stdlib::dynamics::spatial::SpatialVector6;

mod spatial_vector6 {
    use super::*;

    #[test]
    fn zero_is_six_zeros() {
        let z = SpatialVector6::zero();
        assert_eq!(z.as_array(), [0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn from_array_round_trips_via_as_array() {
        let a = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let v = SpatialVector6::from_array(a);
        assert_eq!(v.as_array(), a);
    }

    #[test]
    fn angular_is_first_three_linear_is_last_three() {
        let v = SpatialVector6::from_array([1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        assert_eq!(v.angular(), [1.0, 2.0, 3.0]);
        assert_eq!(v.linear(), [4.0, 5.0, 6.0]);
    }

    #[test]
    fn from_angular_linear_round_trips() {
        let v = SpatialVector6::from_angular_linear([1.0, 2.0, 3.0], [4.0, 5.0, 6.0]);
        assert_eq!(v.as_array(), [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        assert_eq!(v.angular(), [1.0, 2.0, 3.0]);
        assert_eq!(v.linear(), [4.0, 5.0, 6.0]);
    }
}
