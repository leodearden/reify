//! Zienkiewicz-Zhu superconvergent patch-recovery error estimator for
//! tetrahedral P1 linear elastostatics.
//!
//! See PRD `docs/prds/v0_4/a-posteriori-error-estimation.md`, Resolved
//! §"Error indicator" + Task decomposition #1 (task 2996).
//!
//! # Scope
//!
//! Pure-Rust kernel math primitives for the Z-Z error indicator over the v0.3
//! per-element stress field from kernel task #2920. Does NOT plumb into
//! `ElasticResult` (task A3) and does NOT touch the refinement loop (task A2).
//!
//! # Public surface
//!
//! - [`ZzIndicator`] — output carrier: per-element η_e and global relative
//!   energy error η_global.
//! - [`compute_zz_indicator`] — entry point: given a per-element stress field
//!   (as `&[StressElement<'_>]` from task 2920), a mesh for n_nodes, and
//!   material parameters, returns the Z-Z indicator.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constitutive::IsotropicElastic;
    use crate::result::StressElement;

    /// Surface compile pin — confirms that `ZzIndicator` is constructible as
    /// a struct literal and that `compute_zz_indicator` has the expected
    /// function-item signature. Mirrors the doctest in `lib.rs` (Task 2996
    /// block) so any signature drift trips both the doctest and this test.
    #[test]
    fn surface_compile_pin_for_zz_indicator_struct_and_compute_function() {
        let _zz = ZzIndicator {
            per_element: vec![0.5_f64],
            global_relative_energy_error: 0.05_f64,
        };
        let _: fn(
            &[StressElement<'_>],
            &reify_types::VolumeMesh,
            &IsotropicElastic,
        ) -> ZzIndicator = compute_zz_indicator;
    }
}
