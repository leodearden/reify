//! Shift-invert Lanczos + dense generalized eigensolver kernel.
//!
//! # PRD reference
//!
//! `docs/prds/v0_5/buckling-eigensolver.md` §5 eigensolver kernel contract;
//! §13 phase 2 task β.
//!
//! # Scope
//!
//! This module provides two pure-function kernel primitives for the generalized
//! symmetric eigenproblem `K φ = λ B φ`:
//!
//! - [`solve_eigen_dense`] — dense QZ path via `faer::linalg::gevd::gevd_real`
//! - [`solve_eigen_shift_invert`] — shift-invert Lanczos via sparse Cholesky +
//!   `faer::matrix_free::eigen::partial_self_adjoint_eigen`
//!
//! Both functions are neutral on the sign convention of (K, B): the
//! buckling-specific sign flip `B = −K_g` is the responsibility of the caller
//! (task δ/ε).  The trampoline layer also owns mode-string routing
//! (`BucklingOptions.mode`), cancellation hooks, and OpaqueState caching.
//!
//! # Design decisions
//!
//! See `plan.json` design_decisions entries for rationale on: pure-function
//! surface, generic (K, B) sign convention, panic-on-SPD-violation, deterministic
//! start vector, and `faer::Mat<f64>` eigenvector storage.
