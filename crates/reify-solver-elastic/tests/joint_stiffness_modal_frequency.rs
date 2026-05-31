//! Headline acceptance test for task 3868 (κ): PRD compliant-joints-flexures.md §10.1 row 8.
//!
//! A lumped mass `m` on a Howell-cantilever flexure of stiffness `k` has first-mode
//! frequency `f₁ = (1/2π)·√(k/m)` within the PRD's 2% band.
//!
//! Implementation note: faer's dense QZ algorithm (used by `solve_eigen_dense`) requires
//! n ≥ 2 for its scratch-buffer allocation. The test therefore uses a 2-DOF uncoupled
//! spring-mass system: DOF 0 is the Howell-cantilever mass (lower frequency), DOF 1 is
//! a stiff "anchor" DOF with much higher frequency. For a block-diagonal K = diag(k, k₁)
//! and M = diag(m, m₁) the eigenvalues are still closed-form exact: λᵢ = kᵢ/mᵢ,
//! so the PRD 2% band is met with ~machine-precision margin.

use std::f64::consts::PI;

use faer::sparse::{SparseRowMat, Triplet};
use reify_solver_elastic::{
    EigenSolverOptions, JointStiffness, add_joint_stiffness, solve_eigen_dense,
};

#[test]
fn howell_cantilever_first_mode_frequency() {
    // Howell compliant-mechanism cantilever geometry (steel, representative values).
    // PRD compliant-joints-flexures.md §5.1 / §10.1 row 8 cantilever geometry.
    // The cantilever ctor (γ) is out of scope; k is computed inline so the test
    // exercises the K-assembly + eigensolve seam directly.
    let e = 200e9_f64; // Young's modulus [Pa] — representative steel
    let l = 0.020_f64; // beam length [m]
    let b = 0.005_f64; // beam width [m]
    let h = 0.0005_f64; // beam thickness [m]
    let i_sect = b * h.powi(3) / 12.0; // second moment of area [m⁴]
    let k = 2.65 * e * i_sect / l; // Howell cantilever stiffness [N/m]

    let m_load = 0.5_f64; // lumped tip mass [kg]

    // Second DOF: a stiff anchor with a much higher natural frequency (f₁ >> f₀)
    // so that DOF 0's eigenvalue is the smallest (first mode).
    let k_anchor = 1.0e6_f64; // [N/m] — stiff anchor spring
    let m_anchor = 0.001_f64; // [kg]   — light anchor mass
    // f_anchor = √(k_anchor/m_anchor)/(2π) ≈ 5.0 kHz >> f_cantilever ≈ 0.26 Hz.

    // Build a 2×2 zero K (no stored entries) and inject both spring rates.
    // faer's dense QZ algorithm requires n ≥ 2 for its scratch-buffer allocation;
    // the 2-DOF uncoupled system has closed-form eigenvalues λᵢ = kᵢ/mᵢ (exact).
    let k_zero: SparseRowMat<usize, f64> =
        SparseRowMat::try_new_from_triplets(2, 2, &[]).unwrap();
    let k_j = add_joint_stiffness(
        &k_zero,
        &[
            JointStiffness { dof: 0, stiffness: k },
            JointStiffness { dof: 1, stiffness: k_anchor },
        ],
    );

    // Build the 2×2 lumped mass matrix.
    let m_trips: Vec<Triplet<usize, usize, f64>> = vec![
        Triplet::new(0, 0, m_load),
        Triplet::new(1, 1, m_anchor),
    ];
    let m: SparseRowMat<usize, f64> =
        SparseRowMat::try_new_from_triplets(2, 2, &m_trips).unwrap();

    // Solve the generalized eigenproblem K φ = λ M φ; ask for both modes.
    let opts = EigenSolverOptions { n_modes: 2, ..Default::default() };
    let result = solve_eigen_dense(&k_j, &m, opts);

    // For the uncoupled block-diagonal system, λ₀ = k/m_load and λ₁ = k_anchor/m_anchor.
    // eigenvalues are returned ascending by |λ|; since f_cantilever << f_anchor, λ₀ < λ₁.
    assert!(
        result.eigenvalues.len() >= 1,
        "expected at least 1 eigenvalue, got {}",
        result.eigenvalues.len()
    );
    let lambda = result.eigenvalues[0];
    let f_computed = lambda.sqrt() / (2.0 * PI);
    let f_expected = (k / m_load).sqrt() / (2.0 * PI);

    let rel_err = ((f_computed - f_expected) / f_expected).abs();
    assert!(
        rel_err <= 0.02,
        "first-mode frequency relative error {rel_err:.2e} exceeds PRD 2% band: \
         f_computed = {f_computed:.6} Hz, f_expected = {f_expected:.6} Hz \
         (k = {k:.4e} N/m, m = {m_load} kg)",
    );
}
