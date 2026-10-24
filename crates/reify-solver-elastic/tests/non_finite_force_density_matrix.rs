//! A force-density matrix `D` with a NaN or infinite entry has no dense
//! spectrum. Every public tensegrity kernel that classifies `D`'s spectrum
//! reports that as a typed `Spectrum(SpectrumError::NonFinite { row, col })`
//! naming the first non-finite entry, never as a panic or a silently wrong
//! verdict.

#[allow(dead_code)] // each test binary uses a subset of the shared fixtures
mod common;
use common::{analytic_combined_q, canonical_prism, triplex_topology};

use reify_solver_elastic::{SpectrumError, StabilityError, analyze_prestress_stability};

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
