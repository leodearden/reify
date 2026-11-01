//! A force-density matrix `D` with a NaN or infinite entry has no dense
//! spectrum. Every public tensegrity kernel that classifies `D`'s spectrum
//! reports that as a typed `Spectrum(SpectrumError::NonFinite { row, col })`
//! naming the first non-finite entry, never as a panic or a silently wrong
//! verdict.

#[allow(dead_code)] // each test binary uses a subset of the shared fixtures
mod common;
use common::{analytic_combined_q, canonical_prism, prism_surfaces, triplex_topology};

use reify_solver_elastic::{
    ForceDensitySpec, FreeFormError, SpectrumError, StabilityError, analyze_prestress_stability,
    form_find_free, form_find_free_surfaces,
};

#[test]
fn prestress_stability_reports_a_nan_force_density_matrix_as_a_spectrum_error() {
    let (members, _) = triplex_topology();
    let mut q = analytic_combined_q(0.0);
    // Strut (0, 4): D[0,0] is the first entry it poisons in column-major order.
    q[0] = f64::NAN;
    assert_eq!(
        analyze_prestress_stability(&canonical_prism(), &members, &q),
        Err(StabilityError::Spectrum(SpectrumError::NonFinite {
            row: 0,
            col: 0
        })),
    );
}

/// The closed-form prism self-stress scaled by `1e308`: every density is finite
/// and sign-admissible (`|q| ≤ 1.733e308 < f64::MAX`), yet node 0's diagonal
/// `−1.732e308 + 1e308 + 1e308 + 1.732e308` overflows to `+inf`, so `D[0,0]`
/// is `D`'s first non-finite entry.
fn overflowing_prism_q() -> Vec<f64> {
    analytic_combined_q(0.0).iter().map(|q| q * 1e308).collect()
}

#[test]
fn free_standing_form_find_reports_an_overflowed_d_as_a_spectrum_error() {
    let (members, kinds) = triplex_topology();
    assert_eq!(
        form_find_free(
            &canonical_prism(),
            &members,
            &kinds,
            &ForceDensitySpec::Explicit(overflowing_prism_q()),
        )
        .unwrap_err(),
        FreeFormError::Spectrum(SpectrumError::NonFinite { row: 0, col: 0 }),
    );
}

#[test]
fn combined_free_standing_form_find_reports_an_overflowed_d_as_a_spectrum_error() {
    let (members, kinds) = triplex_topology();
    assert_eq!(
        form_find_free_surfaces(
            &canonical_prism(),
            &members,
            &kinds,
            &prism_surfaces(),
            &[0.05, 0.05],
            &ForceDensitySpec::Explicit(overflowing_prism_q()),
        )
        .unwrap_err(),
        FreeFormError::Spectrum(SpectrumError::NonFinite { row: 0, col: 0 }),
    );
}
