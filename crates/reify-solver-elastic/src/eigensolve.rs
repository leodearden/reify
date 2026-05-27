//! Shift-invert Lanczos + dense generalized eigensolver kernel.
//!
//! # PRD reference
//!
//! `docs/prds/v0_5/buckling-eigensolver.md` §5 eigensolver kernel contract;
//! §13 phase 2 task β.
//!
//! # Scope
//!
//! This module provides kernel primitives for the generalized symmetric
//! eigenproblem `K φ = λ M φ`:
//!
//! - [`solve_eigen_dense`] — dense QZ path via `faer::linalg::gevd::gevd_real`
//! - [`solve_eigen_shift_invert`] — shift-invert Lanczos via sparse Cholesky +
//!   `faer::matrix_free::eigen::partial_self_adjoint_eigen`; falls back to
//!   dense when the Krylov window would exceed the problem dimension.
//! - [`lanczos_shift_invert`] — generic Lanczos core operating over arbitrary
//!   [`StiffnessOp`] / [`MetricOp`] operator pairs; no dense fallback (caller
//!   is responsible for small-problem dispatch).
//!
//! Both concrete functions are neutral on the sign convention of (K, M): the
//! buckling-specific sign flip `M = −K_g` is the responsibility of the caller
//! (task δ/ε).  The trampoline layer also owns mode-string routing
//! (`BucklingOptions.mode`), cancellation hooks, and OpaqueState caching.
//!
//! # Dual-consumer pattern
//!
//! The buckling pipeline (`buckling_kernel.rs`) calls
//! `solve_eigen_shift_invert(&k_free, &neg_k_g_free, opts)` — unchanged.
//! The modal-analysis pipeline (task 3819) may call `lanczos_shift_invert`
//! directly with custom `StiffnessOp`/`MetricOp` implementations (e.g.
//! matrix-free K, lumped diagonal M) without going through the sparse wrapper.
//!
//! # Design decisions
//!
//! See `plan.json` design_decisions entries for rationale on: pure-function
//! surface, generic (K, M) sign convention, panic-on-SPD-violation, deterministic
//! start vector, and `faer::Mat<f64>` eigenvector storage.

use faer::{Col, Conj, Mat, Par, Side};
use faer::dyn_stack::{MemBuffer, MemStack, StackReq};
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
    /// Maximum number of Lanczos **thick-restart cycles** (not inner Krylov
    /// iterations) for the shift-invert path; must be ≥ 1.
    ///
    /// One restart cycle expands and compresses the Krylov subspace up to
    /// `max_dim`, so the inner iteration count is roughly
    /// `max_iters · max_dim` — set this knob accordingly.  Mirrors faer's
    /// `PartialEigenParams.max_restarts`; do not extrapolate the value from
    /// `CgResult::iterations` (which counts inner iterations).
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
// Contract guard (shared by the sparse-wrapper entry points)
// ---------------------------------------------------------------------------

/// Validate preconditions shared by both sparse-wrapper solver entry points.
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
// Operator-pair traits
// ---------------------------------------------------------------------------

/// Apply K⁻¹ in place on a vector (or multi-vector).
///
/// Implementations wrap a pre-computed factorization of the stiffness matrix K.
/// The operator is assumed to be self-adjoint and SPD, as required by the
/// shift-invert Lanczos method.
///
/// # Design
///
/// Separate from [`MetricOp`] because the two operators have fundamentally
/// different kernels: K⁻¹ requires a factorization + back-solve (in-place);
/// M is a forward matvec (possibly sparse CSR, lumped diagonal, or
/// matrix-free).  See `plan.json` design_decisions for full rationale.
///
/// The `Sync` supertrait is required because [`lanczos_shift_invert`] wraps
/// the operator pair in a [`faer::matrix_free::LinOp`] implementor, which
/// itself requires `Sync + Debug` (faer-0.24).
pub trait StiffnessOp: Sync {
    /// Problem dimension n.
    fn n(&self) -> usize;
    /// Solve K · out = out in place (overwrites `out` with K⁻¹ · out).
    fn solve_in_place(&self, out: MatMut<'_, f64>);
}

/// Apply the mass / metric matrix M as a forward matvec.
///
/// Implementations may use sparse CSR matvec, a diagonal lumped mass, or a
/// matrix-free assembly routine.
///
/// The `Sync` supertrait is required because [`lanczos_shift_invert`] wraps
/// the operator pair in a [`faer::matrix_free::LinOp`] implementor, which
/// itself requires `Sync + Debug` (faer-0.24).
pub trait MetricOp: Sync {
    /// Problem dimension n.
    fn n(&self) -> usize;
    /// Compute out ← M · rhs.
    fn apply(
        &self,
        out: MatMut<'_, f64>,
        rhs: MatRef<'_, f64>,
        par: Par,
        stack: &mut MemStack,
    );
    /// Scratch requirement for `apply`; passed through to
    /// `partial_self_adjoint_eigen_scratch`.
    fn apply_scratch(&self, rhs_ncols: usize, par: Par) -> StackReq;
}

// ---------------------------------------------------------------------------
// Sparse adapters (zero-cost borrowed-reference wrappers)
// ---------------------------------------------------------------------------

/// Zero-cost adapter: wraps a sparse Cholesky factor as a [`StiffnessOp`].
///
/// Field layout: one fat pointer (`&Llt`) + one `usize` — equivalent to the
/// former `ShiftInvertOp` field layout.  No heap allocation or matrix copy.
pub struct SparseStiffnessOp<'a> {
    pub llt: &'a Llt<usize, f64>,
    pub n: usize,
}

impl StiffnessOp for SparseStiffnessOp<'_> {
    #[inline]
    fn n(&self) -> usize {
        self.n
    }

    #[inline]
    fn solve_in_place(&self, out: MatMut<'_, f64>) {
        SolveCore::<f64>::solve_in_place_with_conj(self.llt, Conj::No, out);
    }
}

/// Zero-cost adapter: wraps a sparse CSR matrix as a [`MetricOp`].
///
/// Field layout: one `SparseRowMatRef` fat pointer — no copy, no allocation.
pub struct SparseMetricOp<'a> {
    pub m: SparseRowMatRef<'a, usize, f64>,
}

impl MetricOp for SparseMetricOp<'_> {
    #[inline]
    fn n(&self) -> usize {
        self.m.nrows()
    }

    #[inline]
    fn apply(
        &self,
        out: MatMut<'_, f64>,
        rhs: MatRef<'_, f64>,
        par: Par,
        stack: &mut MemStack,
    ) {
        LinOp::<f64>::apply(&self.m, out, rhs, par, stack);
    }

    #[inline]
    fn apply_scratch(&self, _rhs_ncols: usize, _par: Par) -> StackReq {
        // SparseRowMatRef::apply is scratch-free.
        StackReq::EMPTY
    }
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
/// - See [`check_eigen_options_and_shapes`] for option/shape contract guards.
/// - Panics with `"eigensolve: gevd_real failed on dense (K, B) pair"` if
///   faer's `gevd_real` returns an error.  In practice this fires only for
///   genuinely ill-conditioned (K, B) pairs (e.g. both nearly singular, or
///   B = 0 to machine precision); the routine handles benign degenerate β
///   internally by filtering eigenvalues, so the panic indicates a
///   pre-decomposition QZ breakdown rather than a near-singular eigenvalue.
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
// Generic shift-invert Lanczos core
// ---------------------------------------------------------------------------

/// Internal composite operator: K⁻¹ · M · v.
///
/// Used inside the Lanczos loop to invert the spectrum of `K φ = λ M φ`.
/// The Krylov method finds the largest |μ| of `K⁻¹ M φ = μ φ` (μ = 1/λ),
/// which correspond to the smallest |λ|.
struct CompositeShiftInvertOp<'a, K: StiffnessOp, M: MetricOp> {
    k_op: &'a K,
    m_op: &'a M,
    n: usize,
}

impl<K: StiffnessOp, M: MetricOp> core::fmt::Debug for CompositeShiftInvertOp<'_, K, M> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "CompositeShiftInvertOp(n={})", self.n)
    }
}

impl<K: StiffnessOp, M: MetricOp> LinOp<f64> for CompositeShiftInvertOp<'_, K, M> {
    #[inline]
    fn nrows(&self) -> usize {
        self.n
    }

    #[inline]
    fn ncols(&self) -> usize {
        self.n
    }

    #[inline]
    fn apply_scratch(&self, rhs_ncols: usize, par: Par) -> StackReq {
        // K⁻¹ back-solve allocates its own internal scratch;
        // M matvec scratch is provided by the MetricOp impl.
        self.m_op.apply_scratch(rhs_ncols, par)
    }

    fn apply(
        &self,
        mut out: MatMut<'_, f64>,
        rhs: MatRef<'_, f64>,
        par: Par,
        stack: &mut MemStack,
    ) {
        // Step 1: out ← M · rhs
        self.m_op.apply(out.rb_mut(), rhs, par, stack);
        // Step 2: out ← K⁻¹ · out  (in-place back-solve)
        self.k_op.solve_in_place(out.rb_mut());
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

/// Shift-invert Lanczos eigensolver over arbitrary SPD operator pairs.
///
/// Solves `K φ = λ M φ` using shift-invert Lanczos (σ = 0).  Finds the
/// smallest |λ| by maximizing |μ| = 1/|λ| in the Krylov subspace of
/// `K⁻¹ · M`.
///
/// This is the generic core — it operates over any [`StiffnessOp`] /
/// [`MetricOp`] pair without knowledge of the underlying representation
/// (sparse CSR, matrix-free, lumped diagonal, etc.).  **No dense fallback**:
/// the caller is responsible for small-problem dispatch.  For the common
/// sparse case with automatic dense fallback use [`solve_eigen_shift_invert`].
///
/// # Parameters
///
/// - `k_op`: pre-factored stiffness inverse (e.g. sparse Cholesky via
///   [`SparseStiffnessOp`])
/// - `m_op`: mass / metric matvec (e.g. CSR via [`SparseMetricOp`])
/// - `opts`: solver options (n_modes, tol, max_iters)
///
/// # Panics
///
/// Named-offending-value messages (Task-2544 convention), verified by
/// `lanczos_shift_invert_panics_on_*` tests:
///
/// - `opts.n_modes == 0` → `"EigenSolverOptions.n_modes = 0 is invalid; must be >= 1"`
/// - `opts.tol` not finite or ≤ 0 → `"EigenSolverOptions.tol = … must be a finite positive value"`
/// - `opts.max_iters == 0` → `"EigenSolverOptions.max_iters = 0 is invalid; must be >= 1"`
/// - `k_op.n() != m_op.n()` → `"lanczos_shift_invert: dimension mismatch — k_op.n() = … but m_op.n() = …"`
pub fn lanczos_shift_invert<K: StiffnessOp, M: MetricOp>(
    k_op: &K,
    m_op: &M,
    opts: EigenSolverOptions,
) -> EigenSolverResult {
    // Contract guards for the generic entry point.
    assert!(
        opts.n_modes >= 1,
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
        k_op.n(),
        m_op.n(),
        "lanczos_shift_invert: dimension mismatch — k_op.n() = {} but m_op.n() = {}",
        k_op.n(),
        m_op.n(),
    );

    let n = k_op.n();

    let op = CompositeShiftInvertOp { k_op, m_op, n };

    // Deterministic unit start vector: v₀ = (1/√n) · 1ₙ
    // (PRD §14 tactical default; fixes Lanczos seed for bit-stable test output).
    let v0 = Col::<f64>::from_fn(n, |_| 1.0 / (n as f64).sqrt());

    // Lanczos subspace dimensions.
    // faer's partial_self_adjoint_eigen_imp requires max_dim < n strictly.
    // See solve_eigen_shift_invert comment for the full FAER_MIN_DIM rationale;
    // callers that need the dense-fallback safety net should use the wrapper.
    let min_dim = opts.n_modes;
    let max_dim = (2 * opts.n_modes).max(32).min(n);

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
    // all n_modes eigenvalues.
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

// ---------------------------------------------------------------------------
// Sparse shift-invert wrapper (with dense fallback)
// ---------------------------------------------------------------------------

/// Solve `K φ = λ B φ` via shift-invert Lanczos (σ = 0).
///
/// Factors K via sparse Cholesky, builds [`SparseStiffnessOp`] +
/// [`SparseMetricOp`] adapters, and delegates to [`lanczos_shift_invert`] for
/// the Krylov computation.
///
/// Falls back to [`solve_eigen_dense`] when the Krylov window would exceed
/// the problem dimension (n ≤ `2·FAER_MIN_DIM = 64`, or n_modes too large
/// relative to n) — the dense path does not require n > effective_max_dim.
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

    // Dense-fallback dispatch (sparse-matrix-specific; not in the generic).
    // faer's partial_self_adjoint_eigen_imp requires max_dim < n strictly.
    // The public wrapper silently clamps: max_dim = min(max(params.max_dim,
    //   max(2*MIN_DIM, 2*n_eigval)), n) with MIN_DIM = 32 (a faer constant).
    // For n ≤ 64 (or 2*n_modes ≥ n) max_dim reaches n, causing a panic in the
    // inner thick-restart loop.  Mirror faer's computation here and fall back
    // to the dense path when the Krylov window would hit the problem size.
    // FAER_MIN_DIM mirrors the private MIN_DIM constant from faer-0.24
    // (src/operator/eigen/mod.rs).  If the faer workspace dependency is bumped,
    // re-check this value.  The `shift_invert_no_panic_at_min_dim_boundaries`
    // integration test sweeps every n in 2..=128 to catch silent divergence
    // from faer's actual floor without requiring a recompile.
    const FAER_MIN_DIM: usize = 32; // faer-0.24
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

    // Delegate to the generic Lanczos core via zero-cost adapter pair.
    // The chained matvec+backsolve through the adapters is byte-equivalent to
    // the former ShiftInvertOp composition (same faer calls in same order),
    // so buckling goldens pass bit-for-bit.
    let k_op = SparseStiffnessOp { llt: &llt, n };
    let m_op = SparseMetricOp { m: b.as_ref() };
    lanczos_shift_invert(&k_op, &m_op, opts)
}
