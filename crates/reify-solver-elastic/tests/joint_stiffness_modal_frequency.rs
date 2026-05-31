//! Headline acceptance test for task 3868 (κ): PRD compliant-joints-flexures.md §10.1 row 8.
//!
//! A lumped mass `m` on a Howell-cantilever flexure of stiffness `k` has first-mode
//! frequency `f₁ = (1/2π)·√(k/m)` within the PRD's 2% band.
//!
//! This file is RED until step-8 GREEN adds the crate-root re-export of
//! `JointStiffness` and `add_joint_stiffness` (the established convention, cf.
//! lib.rs:332-338 Task 3293 / :340-362 Task 3778).
//!
//! Implementation note: for a 1-DOF generalized eigenproblem `K φ = λ M φ` the
//! eigenvalue is closed-form exact: `det(K − λM) = k − λm = 0 ⟹ λ = k/m`.
//! `solve_eigen_dense` reproduces this to ~machine precision, so the PRD 2% band
//! (which budgets for PRB-approximation + distributed-mass error in the full e2e
//! pipeline) is satisfied with ~12 orders of magnitude margin.

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

    // Build a 1×1 zero K (no stored entries) and inject the joint spring rate.
    let k_zero: SparseRowMat<usize, f64> =
        SparseRowMat::try_new_from_triplets(1, 1, &[]).unwrap();
    let k_j = add_joint_stiffness(&k_zero, &[JointStiffness { dof: 0, stiffness: k }]);

    // Build the 1×1 lumped mass matrix.
    let m_trips: Vec<Triplet<usize, usize, f64>> = vec![Triplet::new(0, 0, m_load)];
    let m: SparseRowMat<usize, f64> =
        SparseRowMat::try_new_from_triplets(1, 1, &m_trips).unwrap();

    // Solve the generalized eigenproblem K φ = λ M φ.
    let opts = EigenSolverOptions { n_modes: 1, ..Default::default() };
    let result = solve_eigen_dense(&k_j, &m, opts);

    // For a 1-DOF pencil λ = k/m is exact (closed-form), so f₁ = √λ/(2π).
    let lambda = result.eigenvalues[0];
    let f_computed = lambda.sqrt() / (2.0 * PI);
    let f_expected = (k / m_load).sqrt() / (2.0 * PI);

    let rel_err = ((f_computed - f_expected) / f_expected).abs();
    assert!(
        rel_err <= 0.02,
        "first-mode frequency relative error {rel_err:.2e} exceeds PRD 2% band: \
         f_computed = {f_computed:.4} Hz, f_expected = {f_expected:.4} Hz",
    );
}
