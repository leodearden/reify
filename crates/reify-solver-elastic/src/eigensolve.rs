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

use faer::{Col, Conj, Mat, Par, Side};
use faer::dyn_stack::{MemBuffer, MemStack};
use faer::linalg::gevd::{ComputeEigenvectors, gevd_real, gevd_scratch};
use faer::linalg::solvers::SolveCore;
use faer::mat::{MatMut, MatRef};
use faer::matrix_free::LinOp;
use faer::matrix_free::eigen::{
    PartialEigenParams, partial_self_adjoint_eigen, partial_self_adjoint_eigen_scratch,
};
use faer::sparse::{SparseRowMat, SparseRowMatRef};
use faer::sparse::linalg::solvers::Llt;
use faer::reborrow::ReborrowMut;

/// Options controlling the eigensolver kernel.
///
/// # Defaults
///
/// `n_modes = 10`, `tol = 1e-8`, `max_iters = 1000`, `sigma = 0.0`.
/// Per PRD §4 BucklingOptions defaults.
#[derive(Debug, Clone)]
pub struct EigenSolverOptions {
    /// Number of eigenmode pairs to compute (must be ≥ 1).
    pub n_modes: usize,
    /// Convergence tolerance for iterative paths (must be finite and > 0).
    pub tol: f64,
    /// Maximum number of Lanczos restarts for the shift-invert path (≥ 1).
    pub max_iters: usize,
    /// Shift σ (reserved for shifted-inverse formulation; currently 0.0).
    pub sigma: f64,
}

impl Default for EigenSolverOptions {
    fn default() -> Self {
        Self {
            n_modes: 10,
            tol: 1e-8,
            max_iters: 1000,
            sigma: 0.0,
        }
    }
}

/// Result of an eigensolver kernel call.
pub struct EigenSolverResult {
    /// Eigenvalues sorted ascending by |λ|, length = number of converged modes.
    pub eigenvalues: Vec<f64>,
    /// Column-major eigenvector matrix of shape `n × eigenvalues.len()`.
    pub eigenvectors: Mat<f64>,
    /// Number of eigenvalues converged by the underlying solver.
    ///
    /// For the **dense path** this is always `0` — the direct path has no
    /// iterative budget to report (the full spectrum is computed in one pass).
    /// For the **shift-invert path** this equals `info.n_converged_eigen` from
    /// faer's `partial_self_adjoint_eigen` — the count of Krylov eigenpairs
    /// that satisfied the tolerance criterion.  Normally equals
    /// `eigenvalues.len()`; may exceed it only in the rare case that some
    /// converged Krylov eigenvalues were near-zero and filtered out.
    pub n_converged: usize,
    /// `true` iff all requested `n_modes` eigenvalues were returned
    /// (`eigenvalues.len() == n_modes`).
    pub converged: bool,
}

// ---------------------------------------------------------------------------
// Contract guard (shared by both entry points)
// ---------------------------------------------------------------------------

/// Validate preconditions shared by both solver entry points.
///
/// Panics with named-offending-value messages matching the `solve_cg` style
/// (Task-2544 contract-explicitness convention).
fn check_eigen_options_and_shapes(
    k: &SparseRowMat<usize, f64>,
    b: &SparseRowMat<usize, f64>,
    opts: &EigenSolverOptions,
) {
    assert!(
        opts.n_modes > 0,
        "EigenSolverOptions.n_modes = {} is invalid; must be >= 1",
        opts.n_modes,
    );
    assert!(
        opts.tol.is_finite() && opts.tol > 0.0,
        "EigenSolverOptions.tol = {} must be a finite positive value",
        opts.tol,
    );
    assert!(
        opts.max_iters >= 1,
        "EigenSolverOptions.max_iters = 0 is invalid; must be >= 1",
    );
    assert_eq!(
        k.nrows(),
        k.ncols(),
        "K must be square: k.nrows() = {} but k.ncols() = {}",
        k.nrows(),
        k.ncols(),
    );
    assert!(
        b.nrows() == k.nrows() && b.ncols() == k.ncols(),
        "B must match K dimensions: b = {}×{} but K = {}×{}",
        b.nrows(),
        b.ncols(),
        k.nrows(),
        k.ncols(),
    );
}

// ---------------------------------------------------------------------------
// Dense path
// ---------------------------------------------------------------------------

/// Solve the generalized symmetric eigenproblem `K φ = λ B φ` via dense QZ.
///
/// Densifies K and B, calls `faer::linalg::gevd::gevd_real`, recovers
/// `λ_i = S_re[i] / beta[i]` (skipping near-zero or infinite beta), sorts
/// ascending by `|λ|`, and returns the smallest `n_modes`.
///
/// Sets `n_converged = 0` (direct path; no iterative budget consumed).
/// Sets `converged = (n_take == opts.n_modes)` — `false` only when B is
/// nearly singular and too many eigenvalues are filtered out.
///
/// # Panics
///
/// See [`check_eigen_options_and_shapes`].
pub fn solve_eigen_dense(
    k: &SparseRowMat<usize, f64>,
    b: &SparseRowMat<usize, f64>,
    opts: EigenSolverOptions,
) -> EigenSolverResult {
    check_eigen_options_and_shapes(k, b, &opts);
    let n = k.nrows();

    // Densify K and B by iterating over stored entries.
    let mut k_dense = Mat::<f64>::zeros(n, n);
    let mut b_dense = Mat::<f64>::zeros(n, n);
    {
        let k_ref = k.as_ref();
        let b_ref = b.as_ref();
        // Hoist symbolic() calls outside the row loop — each call is a
        // lightweight pointer borrow, but recomputing it per row is noisy.
        let k_sym = k_ref.symbolic();
        let b_sym = b_ref.symbolic();
        for i in 0..n {
            let k_cols = k_sym.col_idx_of_row_raw(i);
            let k_vals = k_ref.val_of_row(i);
            for (col_idx, &val) in k_cols.iter().zip(k_vals.iter()) {
                let j = *col_idx;
                k_dense[(i, j)] = val;
            }
            let b_cols = b_sym.col_idx_of_row_raw(i);
            let b_vals = b_ref.val_of_row(i);
            for (col_idx, &val) in b_cols.iter().zip(b_vals.iter()) {
                let j = *col_idx;
                b_dense[(i, j)] = val;
            }
        }
    }

    // Allocate result containers.
    let mut s_re = Col::<f64>::zeros(n);
    let mut s_im = Col::<f64>::zeros(n);
    let mut beta_col = Col::<f64>::zeros(n);
    let mut u_right = Mat::<f64>::zeros(n, n);

    // Allocate scratch and call gevd_real.
    let scratch_req = gevd_scratch::<f64>(
        n,
        ComputeEigenvectors::No,
        ComputeEigenvectors::Yes,
        Par::Seq,
        Default::default(),
    );
    let mut buf = MemBuffer::new(scratch_req);
    let stack = MemStack::new(&mut buf);

    gevd_real(
        k_dense.as_mut(),
        b_dense.as_mut(),
        s_re.as_diagonal_mut(),
        s_im.as_diagonal_mut(),
        beta_col.as_diagonal_mut(),
        None,
        Some(u_right.as_mut()),
        Par::Seq,
        stack,
        Default::default(),
    )
    .expect("eigensolve: gevd_real failed on dense (K, B) pair");

    // Recover eigenvalues: λ_i = S_re[i] / beta[i]; skip degenerate beta.
    let mut pairs: Vec<(f64, usize)> = (0..n)
        .filter_map(|i| {
            let b_i = beta_col[i];
            if b_i.abs() < f64::MIN_POSITIVE {
                return None;
            }
            let lambda = s_re[i] / b_i;
            if lambda.is_finite() { Some((lambda, i)) } else { None }
        })
        .collect();

    // Sort ascending by |λ|; stable sort preserves relative order of equal |λ|.
    pairs.sort_by(|a, b| a.0.abs().total_cmp(&b.0.abs()));

    let n_take = pairs.len().min(opts.n_modes);
    let eigenvalues: Vec<f64> = pairs[..n_take].iter().map(|&(lam, _)| lam).collect();

    let mut eigenvectors = Mat::<f64>::zeros(n, n_take);
    for (out_col, &(_, src_col)) in pairs[..n_take].iter().enumerate() {
        // Column-major faer storage: copy whole column slice in one memcpy.
        eigenvectors
            .col_as_slice_mut(out_col)
            .copy_from_slice(u_right.col_as_slice(src_col));
    }

    EigenSolverResult {
        eigenvalues,
        eigenvectors,
        n_converged: 0,
        converged: n_take == opts.n_modes,
    }
}

// ---------------------------------------------------------------------------
// Shift-invert path
// ---------------------------------------------------------------------------

/// Shift-invert linear operator: applies `K⁻¹ · B · v`.
///
/// Used inside the Lanczos loop to invert the spectrum of `K φ = λ B φ`.
/// The Krylov method finds the largest |μ| of `K⁻¹ B φ = μ φ` (μ = 1/λ),
/// which correspond to the smallest |λ|.
struct ShiftInvertOp<'a> {
    llt: &'a Llt<usize, f64>,
    b_ref: SparseRowMatRef<'a, usize, f64>,
    n: usize,
}

impl core::fmt::Debug for ShiftInvertOp<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "ShiftInvertOp(n={})", self.n)
    }
}

impl LinOp<f64> for ShiftInvertOp<'_> {
    #[inline]
    fn nrows(&self) -> usize {
        self.n
    }

    #[inline]
    fn ncols(&self) -> usize {
        self.n
    }

    #[inline]
    fn apply_scratch(&self, _rhs_ncols: usize, _par: Par) -> faer::dyn_stack::StackReq {
        // SparseRowMatRef::apply_scratch returns EMPTY;
        // Llt::solve_in_place_with_conj allocates its own internal scratch.
        faer::dyn_stack::StackReq::EMPTY
    }

    fn apply(
        &self,
        mut out: MatMut<'_, f64>,
        rhs: MatRef<'_, f64>,
        par: Par,
        stack: &mut MemStack,
    ) {
        // Step 1: out ← B · rhs  (CSR LinOp blanket impl, scratch-free)
        LinOp::<f64>::apply(&self.b_ref, out.rb_mut(), rhs, par, stack);
        // Step 2: out ← K⁻¹ · out  (Cholesky back-solve, in-place)
        SolveCore::<f64>::solve_in_place_with_conj(self.llt, Conj::No, out.rb_mut());
    }

    fn conj_apply(
        &self,
        out: MatMut<'_, f64>,
        rhs: MatRef<'_, f64>,
        par: Par,
        stack: &mut MemStack,
    ) {
        // Real symmetric: conj_apply ≡ apply.
        self.apply(out, rhs, par, stack);
    }
}

/// Solve `K φ = λ B φ` via shift-invert Lanczos (σ = 0).
///
/// Factors K via sparse Cholesky, builds a [`ShiftInvertOp`] that applies
/// `K⁻¹ · B`, and runs `partial_self_adjoint_eigen` to recover the Krylov
/// eigenvalues μ with the largest |μ| (= smallest |λ| = 1/|μ|).
///
/// Returns up to `info.n_converged_eigen` eigenvalues sorted ascending by |λ|.
/// If `n_converged_eigen >= n_modes`, `converged = true`; otherwise
/// `converged = false` and the partial result is returned.
///
/// # Panics
///
/// - K is not SPD (Cholesky failure → panic with descriptive message, matching
///   Task-2544 panic-on-contract convention)
/// - See also [`check_eigen_options_and_shapes`]
pub fn solve_eigen_shift_invert(
    k: &SparseRowMat<usize, f64>,
    b: &SparseRowMat<usize, f64>,
    opts: EigenSolverOptions,
) -> EigenSolverResult {
    check_eigen_options_and_shapes(k, b, &opts);
    let n = k.nrows();

    // Factor K via sparse Cholesky (panics if not SPD per Task-2544 convention).
    let llt = k
        .sp_cholesky(Side::Lower)
        .expect("eigensolve: K must be SPD; sp_cholesky failed — check that BCs have been applied");

    let op = ShiftInvertOp {
        llt: &llt,
        b_ref: b.as_ref(),
        n,
    };

    // Deterministic unit start vector: v₀ = (1/√n) · 1ₙ
    // (PRD §14 tactical default; fixes Lanczos seed for bit-stable test output).
    let v0 = Col::<f64>::from_fn(n, |_| 1.0 / (n as f64).sqrt());

    // Lanczos subspace dimensions.
    // faer's partial_self_adjoint_eigen_imp requires max_dim < n strictly.
    // The public wrapper silently clamps: max_dim = min(max(params.max_dim,
    //   max(2*MIN_DIM, 2*n_eigval)), n) with MIN_DIM = 32 (a faer constant).
    // For n ≤ 64 (or 2*n_modes ≥ n) max_dim reaches n, causing a panic in the
    // inner thick-restart loop.  Mirror faer's computation here and fall back
    // to the dense path when the Krylov window would hit the problem size.
    // FAER_MIN_DIM mirrors the private MIN_DIM constant from faer-0.24
    // (src/operator/eigen/mod.rs).  If the faer workspace dependency is bumped,
    // re-check this value.  The `shift_invert_no_panic_at_min_dim_boundaries`
    // integration test probes n ∈ {2, 16, 32, 33, 63, 64, 65} to catch silent
    // divergence from faer's actual floor without requiring a recompile.
    const FAER_MIN_DIM: usize = 32; // faer-0.24
    let min_dim = opts.n_modes;
    let max_dim = (2 * opts.n_modes).max(32).min(n);
    let effective_max_dim = max_dim
        .max(2 * FAER_MIN_DIM)
        .max(2 * opts.n_modes)
        .min(n);

    if effective_max_dim >= n {
        // Problem too small for Lanczos; delegate to the direct dense solver.
        // The dense result already satisfies the EigenSolverResult contract
        // (converged=true, iterations=0, eigenvalues sorted ascending |λ|).
        return solve_eigen_dense(k, b, opts);
    }

    let params = PartialEigenParams {
        min_dim,
        max_dim,
        max_restarts: opts.max_iters,
        ..PartialEigenParams::default()
    };

    // Allocate eigenvector and eigenvalue storage (n_modes slots).
    let mut eigvecs = Mat::<f64>::zeros(n, opts.n_modes);
    let mut eigvals_mu = vec![0.0_f64; opts.n_modes];

    let scratch_req = partial_self_adjoint_eigen_scratch::<f64>(
        &op as &dyn LinOp<f64>,
        opts.n_modes,
        Par::Seq,
        params,
    );
    let mut buf = MemBuffer::new(scratch_req);
    let stack = MemStack::new(&mut buf);

    let info = partial_self_adjoint_eigen(
        eigvecs.as_mut(),
        &mut eigvals_mu,
        &op as &dyn LinOp<f64>,
        v0.as_ref(),
        opts.tol,
        Par::Seq,
        stack,
        params,
    );

    let n_conv = info.n_converged_eigen;

    // Convert μ → λ = 1/μ for the converged modes only.
    let mut pairs: Vec<(f64, usize)> = (0..n_conv)
        .filter_map(|i| {
            let mu = eigvals_mu[i];
            if mu.abs() < f64::MIN_POSITIVE {
                return None;
            }
            let lambda = 1.0 / mu;
            if lambda.is_finite() { Some((lambda, i)) } else { None }
        })
        .collect();

    // Sort ascending by |λ| (stable).
    pairs.sort_by(|a, b| a.0.abs().total_cmp(&b.0.abs()));

    let n_take = pairs.len().min(opts.n_modes);
    // Track what the caller actually receives: converged iff we hand back
    // all n_modes eigenvalues.  Tighter than `n_conv >= n_modes` alone,
    // since filter_map can drop near-zero μ even when n_conv == n_modes.
    let converged = n_take == opts.n_modes;
    let eigenvalues: Vec<f64> = pairs[..n_take].iter().map(|&(lam, _)| lam).collect();

    let mut eigenvectors = Mat::<f64>::zeros(n, n_take);
    for (out_col, &(_, src_col)) in pairs[..n_take].iter().enumerate() {
        // Column-major faer storage: copy whole column slice in one memcpy.
        eigenvectors
            .col_as_slice_mut(out_col)
            .copy_from_slice(eigvecs.col_as_slice(src_col));
    }

    EigenSolverResult {
        eigenvalues,
        eigenvectors,
        n_converged: n_conv,
        converged,
    }
}
