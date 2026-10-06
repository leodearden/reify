//! The Cholesky-symmetrized shift-invert operator: the Lanczos core for a
//! pencil `K φ = λ B φ` whose `B` is NOT a scalar multiple of the identity.
//!
//! faer's `partial_self_adjoint_eigen` assumes a Euclidean-symmetric operator,
//! and `(K − σB)⁻¹B` is one only when `B = cI`.  This module hands faer an
//! operator `S` that is Euclidean-symmetric for every symmetric `B` and has
//! the same eigenvalues `μ = 1/(λ − σ)`; [`LanczosMetric`] carries the
//! derivation.  The Krylov method itself is unchanged, as PRD
//! `docs/prds/v0_6/shift-invert-eigensolve.md` §8 requires.

use faer::Par;
use faer::dyn_stack::{MemStack, StackReq};
use faer::mat::{MatMut, MatRef};
use faer::matrix_free::LinOp;
use faer::reborrow::{Reborrow, ReborrowMut};

use super::{
    EigenSolverOptions, EigenSolverResult, MetricOp, StiffnessOp, check_lanczos_options,
    finish_lanczos_result, run_partial_self_adjoint_eigen,
};
use crate::split_cholesky::SplitCholesky;

/// The SPD metric `W = G·Gᵀ` a Cholesky-symmetrized Lanczos runs in.
///
/// For a pencil `(K, B)` with symmetric `B ≠ cI`, `A = (K − σB)⁻¹B` is not
/// Euclidean-symmetric, but `W·A` is for any `W = K − τB`:
///
/// ```text
/// W·(K − σB)⁻¹B = B + (σ − τ)·B(K − σB)⁻¹B
/// ```
///
/// So `S = G⁻¹(W·A)G⁻ᵀ` is Euclidean-symmetric with the same eigenvalues
/// `μ = 1/(λ − σ)`, and `φ = G⁻ᵀy` recovers the pencil's eigenvectors.  Each arm
/// is one choice of τ.  Normative source: `docs/prds/v0_6/shift-invert-eigensolve.md`
/// §6 (2026-09-29 amendment).
#[derive(Clone, Copy)]
pub enum LanczosMetric<'a> {
    /// τ = σ: G factors `K − σB` itself (σ=0 ⇒ `K`), which must be SPD;
    /// `S = G⁻¹BG⁻ᵀ`.  `K` alone need NOT be SPD.
    ShiftedPencil(&'a SplitCholesky),
    /// τ = 0: G factors `K`, which must be SPD; `K − σB` is indefinite and is
    /// applied through `shifted_inverse`; `S = G⁻¹[B + σ·B(K − σB)⁻¹B]G⁻ᵀ`.
    Stiffness {
        k_factor: &'a SplitCholesky,
        shifted_inverse: &'a dyn StiffnessOp,
    },
}

impl LanczosMetric<'_> {
    fn factor(&self) -> &SplitCholesky {
        match *self {
            LanczosMetric::ShiftedPencil(factor) => factor,
            LanczosMetric::Stiffness { k_factor, .. } => k_factor,
        }
    }
}

/// Internal symmetrized operator `S = G⁻¹[B + (σ − τ)·B(K − σB)⁻¹B]G⁻ᵀ`; see
/// [`LanczosMetric`].
struct SymmetrizedShiftInvertOp<'a, M: MetricOp> {
    metric: LanczosMetric<'a>,
    b_op: &'a M,
    sigma: f64,
    n: usize,
}

impl<M: MetricOp> core::fmt::Debug for SymmetrizedShiftInvertOp<'_, M> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "SymmetrizedShiftInvertOp(n={})", self.n)
    }
}

impl<M: MetricOp> LinOp<f64> for SymmetrizedShiftInvertOp<'_, M> {
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
        // Temporaries are heap-allocated per apply; only B's matvec draws on
        // the stack.
        self.b_op.apply_scratch(rhs_ncols, par)
    }

    fn apply(
        &self,
        mut out: MatMut<'_, f64>,
        rhs: MatRef<'_, f64>,
        par: Par,
        stack: &mut MemStack,
    ) {
        let factor = self.metric.factor();
        let mut x = rhs.to_owned();
        factor.solve_factor_transpose_in_place(x.as_mut());
        self.b_op.apply(out.rb_mut(), x.as_ref(), par, stack);
        if let LanczosMetric::Stiffness { shifted_inverse, .. } = self.metric {
            let mut w = out.to_owned();
            shifted_inverse.solve_in_place(w.as_mut());
            self.b_op.apply(x.as_mut(), w.as_ref(), par, stack);
            for j in 0..out.ncols() {
                for i in 0..out.nrows() {
                    out[(i, j)] += self.sigma * x[(i, j)];
                }
            }
        }
        factor.solve_factor_in_place(out.rb_mut());
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

/// Cholesky-symmetrized shift-invert Lanczos for `K φ = λ B φ` with any
/// symmetric `B` — the core [`lanczos_shift_invert`](super::lanczos_shift_invert)
/// cannot serve unless `B = cI`.
///
/// Runs faer's Lanczos on the Euclidean-symmetric `S` of [`LanczosMetric`], then
/// recovers `φ = G⁻ᵀy` and scales each column to unit Euclidean norm.  Like the
/// Euclidean core, `opts.sigma` DESCRIBES the factorizations in `metric` (and
/// the core cannot verify it), there is no dense fallback, and C5 is reported
/// conservatively — the caller that built the factorizations owns C6 and the
/// established provenance.
///
/// # Panics
///
/// The option guards of [`lanczos_shift_invert`](super::lanczos_shift_invert),
/// and a dimension mismatch
/// between `b_op` and the metric factor (or, in the `Stiffness` arm,
/// `shifted_inverse`).
pub fn lanczos_shift_invert_in_metric<M: MetricOp>(
    metric: LanczosMetric<'_>,
    b_op: &M,
    opts: EigenSolverOptions,
) -> EigenSolverResult {
    check_lanczos_options(&opts);
    let n = b_op.n();
    let factor = metric.factor();
    assert_eq!(
        factor.n(),
        n,
        "lanczos_shift_invert_in_metric: dimension mismatch — metric factor n() = {} \
         but b_op.n() = {}",
        factor.n(),
        n,
    );
    if let LanczosMetric::Stiffness { shifted_inverse, .. } = metric {
        assert_eq!(
            shifted_inverse.n(),
            n,
            "lanczos_shift_invert_in_metric: dimension mismatch — shifted_inverse.n() = {} \
             but b_op.n() = {}",
            shifted_inverse.n(),
            n,
        );
    }

    let op = SymmetrizedShiftInvertOp {
        metric,
        b_op,
        sigma: opts.sigma,
        n,
    };
    let ritz = run_partial_self_adjoint_eigen(&op, n, &opts);
    finish_lanczos_result(&op, ritz, &opts, |vectors| {
        recover_pencil_eigenvectors(factor, vectors)
    })
}

/// `φ = G⁻ᵀy`, each column then scaled to unit Euclidean norm.
fn recover_pencil_eigenvectors(factor: &SplitCholesky, mut vectors: MatMut<'_, f64>) {
    factor.solve_factor_transpose_in_place(vectors.rb_mut());
    for j in 0..vectors.ncols() {
        let norm = vectors.rb().col(j).norm_l2();
        for i in 0..vectors.nrows() {
            vectors[(i, j)] /= norm;
        }
    }
}
