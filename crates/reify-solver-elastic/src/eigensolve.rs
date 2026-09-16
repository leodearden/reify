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
//! - [`solve_eigen_dense`] — dense QZ path via `faer::linalg::gevd::gevd_real`;
//!   honors `opts.sigma` as a selection key over the full computed spectrum
//! - [`solve_eigen_shift_invert`] — shift-invert Lanczos via sparse Cholesky +
//!   `faer::matrix_free::eigen::partial_self_adjoint_eigen`; falls back to
//!   dense when the Krylov window would exceed the problem dimension.
//! - [`try_solve_eigen_shift_invert`] — the same solve, returning `None`
//!   instead of panicking when `K` is not SPD, and ONLY then: a non-numeric
//!   Cholesky failure (out of memory / index overflow) still panics (task 6663;
//!   for callers that can legitimately be handed an under-constrained system).
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
//! # Shift contract (C1–C6)
//!
//! Normative source: `docs/prds/v0_6/shift-invert-eigensolve.md` §6. This
//! section is the written spec both implementations are built against; the
//! executable form is `tests/eigensolve_shift_contract.rs`.
//!
//! [`EigenSolverOptions::sigma`] is the shift **in eigenvalue (λ) space** for
//! both callers. Unit conversion — a modal caller's frequency, say — is the
//! CALLER's job, not the eigensolver's (PRD §7 seam table). Handing this
//! function a shift in any other space is a caller bug it cannot detect.
//!
//! - **C1 — σ=0 is the identity.** `sigma == 0.0` produces the same
//!   eigenvalues, the same order and the same code path as before this PRD.
//!   STRUCTURAL, not a tolerance: `λ − 0.0 == λ` bit-exactly in IEEE-754 for
//!   every finite λ (and `|−0.0 − 0.0| == 0.0`), so the selection sort at σ=0
//!   IS the pre-PRD `|λ|` sort, the selected prefix is the same slice, and a
//!   STABLE re-sort of an already-`|λ|`-ascending prefix is a no-op. No
//!   `if sigma == 0.0` branch exists or is needed on the dense path.
//! - **C2 — Selection.** The returned set is the `n_modes` converged
//!   eigenvalues with the smallest `|λ − σ|`.
//! - **C3 — Order.** `eigenvalues` is ascending by `|λ|` — absolute, not
//!   signed, exactly as before this PRD: λ=−2 still sorts before λ=+3. The
//!   `eigenvectors` columns are permuted to match.
//! - **C4 — Back-shift.** Eigenvalues are returned in the original λ space
//!   (`λ = σ + 1/μ` on the Lanczos path), never in shifted or μ space.
//! - **C5 — Provenance.** [`EigenSolverResult::shift_skipped_modes`] reports
//!   whether any eigenvalue of the pencil lies between zero and σ and is absent
//!   from the returned set; [`EigenSolverResult::shift`] carries the σ used.
//!   `false` is reported only when ESTABLISHED (a full-spectrum count, or a
//!   Cholesky success), never assumed. Exact on the dense path, a conservative
//!   boolean on Lanczos. The PRD's "between zero and σ" wording is implemented
//!   verbatim and has a known gap on indefinite pencils — see the known-gap
//!   section on [`EigenSolverResult::shift_skipped_modes`], which must be
//!   settled before ε (#7262) ships a refusal keyed on the flag.
//! - **C6 — Singularity.** A singular or numerically-degenerate `K − σB` is a
//!   typed failure carrying σ — never a panic, never a silently perturbed
//!   solve, never a garbage spectrum.
//!
//! **C2 and C3 are two different rules and no implementation may conflate
//! them.** Selection decides WHICH eigenvalues come back; order decides in WHAT
//! ORDER they are presented. A comparator that fuses them returns the right set
//! in proximity order, so a 300 Hz band inspection reads as 301, 298, 310, 295
//! instead of 295, 298, 301, 310 — the right answer in a form an engineer
//! cannot read (PRD §5.2). They are separate functions here for that reason.
//!
//! ## Per-clause implementation status
//!
//! | Clause | Dense path ([`solve_eigen_dense`]) | Lanczos path |
//! |---|---|---|
//! | C1 | implemented | implemented |
//! | C2 | implemented | #7259 — solves at σ=0 until then, and reports `shift: 0.0` |
//! | C3 | implemented (shared helper) | implemented (same helper) |
//! | C4 | n/a — no shift is ever applied to invert | #7259 |
//! | C5 | implemented, EXACT | established `false` at its own σ=0; #7259 |
//! | C6 | vacuous — no `K − σB` is ever formed | #7259 |
//!
//! Staging α before β means [`solve_eigen_shift_invert`] honors σ for n ≤ 64
//! (dense fallback) and not for larger problems (Lanczos). That divergence is
//! deliberate but it is **not silent**: [`EigenSolverResult::shift`] reports the
//! σ a solve actually used, never the σ it was asked for, so
//! `result.shift == opts.sigma` is a definite caller-side test for whether the
//! shift was honored. Echoing the request over an unshifted answer would be
//! correct-looking provenance describing a solve that never happened — the
//! silent-substitution class this contract exists to close.
//!
//! The dense path honors σ as a SORT-KEY CHANGE AND NOTHING ELSE: `gevd_real`
//! computes the entire spectrum, so no factorization is formed, `K − σB` never
//! exists, and no new failure mode is introduced. That is why it lands first
//! and the Lanczos implementation is held to it. The acceptance criterion for
//! #7259 is `tests/eigensolve_shift_contract.rs` — its σ≠0 arms instantiate the
//! harness functions already there, rather than inventing their own.
//!
//! # Design decisions
//!
//! See `plan.json` design_decisions entries for rationale on: pure-function
//! surface, generic (K, M) sign convention, panic-on-SPD-violation, deterministic
//! start vector, and `faer::Mat<f64>` eigenvector storage.
//!
//! # Debug performance
//!
//! The shift-invert / Lanczos path's debug speed is a **faer build-profile
//! characteristic**, not an algorithmic defect.  Release is fast and correct;
//! debug is catastrophically slow without an explicit profile override because
//! faer's SIMD numeric kernels run unoptimized and with internal
//! `debug_assertions` enabled.
//!
//! Measured timings for the n=80 synthetic pair test
//! (`shift_invert_and_dense_agree_on_80dof_synthetic_pair`):
//!
//! | profile | before fix | after fix (task 4055) |
//! |---------|------------|----------------------|
//! | release | ~0.644 s   | ~0.650 s (unchanged) |
//! | debug   | 300–540 s  | ~0.646 s             |
//!
//! Boundary sweep (`shift_invert_no_panic_at_min_dim_boundaries`,
//! 127 solves n=2..=128): debug ~0.107 s after fix (was 300–540 s).
//!
//! **Fast debug requires** the root workspace `Cargo.toml` to contain:
//!
//! ```toml
//! [profile.dev.package."*"]
//! opt-level = 3
//! debug-assertions = false
//! overflow-checks = false
//!
//! [profile.dev.package.reify-solver-elastic]
//! opt-level = 2   # faer generic kernels monomorphised here; assertions kept on
//! ```
//!
//! If a debug-mode performance regression appears (hundreds of seconds),
//! check those overrides first — they are the most likely culprit.
//!
//! **Full regression (task 4055 s6):** 478 tests pass in both debug and
//! release with no collateral drift (CG solver, assembly, buckling, shell
//! benchmarks all unaffected by the dep opt-level override).

use faer::{Col, Conj, Mat, Par, Side};
use faer::dyn_stack::{MemBuffer, MemStack, StackReq};
use faer::linalg::gevd::{ComputeEigenvectors, gevd_real, gevd_scratch};
use faer::linalg::solvers::SolveCore;
use faer::mat::{MatMut, MatRef};
use faer::matrix_free::LinOp;
use faer::matrix_free::eigen::{
    PartialEigenParams, partial_self_adjoint_eigen, partial_self_adjoint_eigen_scratch,
};
use faer::sparse::linalg::LltError as SparseLltError;
use faer::sparse::{SparseRowMat, SparseRowMatRef};
use faer::sparse::linalg::solvers::{Llt, Lu};
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
    /// Spectral shift σ, in eigenvalue (λ) space.
    ///
    /// Selects the `n_modes` eigenvalues nearest σ rather than nearest zero.
    /// Unit conversion is the caller's job — see the module-level
    /// "Shift contract (C1–C6)" section, which is the normative spec for how
    /// every path must treat this value.
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
    /// Whether any eigenvalue of the pencil lies STRICTLY between zero and
    /// [`shift`](Self::shift) AND is absent from the returned set — i.e.
    /// whether this result is a *window* around σ rather than the bottom of the
    /// spectrum (contract clause C5).
    ///
    /// Per C5, `false` is only ever reported when it has been ESTABLISHED,
    /// never assumed.  Like [`n_converged`](Self::n_converged) the basis differs
    /// per path: [`solve_eigen_dense`] computes the whole spectrum via QZ and so
    /// answers EXACTLY — it compares source indices between the full spectrum
    /// and the selected set — while the **shift-invert path** has only the
    /// Cholesky/LU discriminator, which is a conservative boolean (an exact
    /// count would need an inertia-revealing LDL^T that faer's sparse LU does
    /// not expose).  At σ=0 every path reports `false`, and that `false` is
    /// established rather than assumed: the open interval strictly between 0
    /// and 0 is empty, so no eigenvalue can lie in it.
    ///
    /// # Known gap: an interval rule under an absolute-value order
    ///
    /// The predicate is the PRD §6 wording *verbatim* — "between zero and σ" —
    /// but C3 orders by `|λ|`, so "the first mode" means smallest `|λ|`, and the
    /// two do not coincide when the pencil is indefinite.  With spectrum
    /// {−0.1, 0.5, 1.0}, σ=0.6 and `n_modes=1`, C2 selects {0.5}; nothing lies
    /// in the open interval (0, 0.6) that is absent, so this field is `false` —
    /// yet the true first mode by `|λ|` is −0.1 and it did NOT come back.
    /// Consumers that key a refusal on this flag (ε #7262's `critical_load` /
    /// `safety_factor_buckling` / `first_frequency`) therefore inherit the
    /// precondition that the pencil has no eigenvalue on the far side of zero
    /// from σ.  Indefinite pencils are not hypothetical here: `buckling_kernel`
    /// assembles a `neg_sigma` geometric stiffness for the reversed-load case.
    ///
    /// No caller passes σ≠0 today, so the gap is unreachable.  Closing it means
    /// amending PRD §6 C5 to the predicate that serves the stated purpose —
    /// "some eigenvalue with `|λ|` strictly less than `min |λ|` over the
    /// selected set is absent from it", which subsumes the interval rule for
    /// both signs of σ and is still exact on the dense path — rather than
    /// diverging from the PRD here.  That amendment must be settled before ε
    /// (#7262) ships a refusal keyed on this field.
    pub shift_skipped_modes: bool,
    /// The shift σ actually used for this solve (contract clause C5).
    ///
    /// Carried on the result so a diagnostic can name the offending σ without
    /// the caller re-deriving it from its own options.  In eigenvalue (λ) space,
    /// like [`EigenSolverOptions::sigma`] — unit conversion is the caller's job.
    ///
    /// "Actually used" is load-bearing: this is NOT an echo of
    /// [`EigenSolverOptions::sigma`], and a path that did not honor the
    /// requested σ reports the one it did solve at.  The Lanczos path reports
    /// `0.0` for any request until #7259 makes it honor σ, so
    /// `result.shift == opts.sigma` is a definite caller-side test for whether
    /// the shift was applied — and the reason a caller never has to infer it
    /// from the problem dimension.
    pub shift: f64,
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
    // σ is a live selection key (C2), so a non-finite one is a caller bug that
    // must be loud.  Left unguarded it degrades silently instead: every
    // `(λ − σ).abs()` is NaN, `total_cmp` orders all NaNs equal so the stable
    // sort is a no-op, and the caller receives gevd's arbitrary internal order
    // as if it were the nearest-σ set — well-formed and wrong, which is the
    // silent-substitution class this contract exists to close.  ±∞ collapses
    // the same way (every distance is ∞).
    assert!(
        opts.sigma.is_finite(),
        "EigenSolverOptions.sigma = {} must be finite",
        opts.sigma,
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
/// Implementations wrap a pre-computed factorization of the matrix the caller
/// intends to invert.  On the UNSHIFTED path that matrix is K itself and is
/// SPD; once a shift is applied it is `K − σB`, which is symmetric but may be
/// INDEFINITE — a σ placed above some eigenvalue of the pencil is exactly the
/// case where it is.  So self-adjointness is the standing assumption here;
/// positive-definiteness is not, and an implementation must not rely on it.
/// (That is why [`SparseStiffnessOp`] carries a [`SparseFactorRef`] rather than
/// a Cholesky factor: an indefinite `K − σB` factors through LU instead.)
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

/// A borrowed sparse factorization, of whichever kind the matrix admitted.
///
/// Cholesky is the preferred arm and the only one the unshifted path ever
/// reaches: it is cheaper, and its SUCCESS is itself evidence (by Sylvester's
/// law of inertia) that the factored matrix is positive definite.  LU is the
/// fallback for a `K − σB` that is symmetric but indefinite, where Cholesky
/// necessarily fails with a non-positive pivot.
///
/// Both arms are borrowed references, so this enum is one discriminant plus one
/// fat pointer — no heap allocation and no matrix copy, the same zero-cost
/// shape [`SparseStiffnessOp`] has always had.
pub enum SparseFactorRef<'a> {
    /// `LLᵀ` of a positive-definite matrix.
    Cholesky(&'a Llt<usize, f64>),
    /// `PAQ = LU` of a matrix that is not positive definite.
    Lu(&'a Lu<usize, f64>),
}

/// Zero-cost adapter: wraps a sparse factorization as a [`StiffnessOp`].
///
/// Field layout: one [`SparseFactorRef`] (discriminant + fat pointer) + one
/// `usize`.  No heap allocation or matrix copy.
pub struct SparseStiffnessOp<'a> {
    pub factor: SparseFactorRef<'a>,
    pub n: usize,
}

impl StiffnessOp for SparseStiffnessOp<'_> {
    #[inline]
    fn n(&self) -> usize {
        self.n
    }

    #[inline]
    fn solve_in_place(&self, out: MatMut<'_, f64>) {
        // Both arms issue the SAME faer call with the SAME arguments in the
        // SAME order; only the factor object differs.  The match is a branch,
        // not an arithmetic change, so the Cholesky arm remains byte-equivalent
        // to the pre-enum shape and the σ=0 goldens are untouched.
        match self.factor {
            SparseFactorRef::Cholesky(llt) => {
                SolveCore::<f64>::solve_in_place_with_conj(llt, Conj::No, out);
            }
            SparseFactorRef::Lu(lu) => {
                SolveCore::<f64>::solve_in_place_with_conj(lu, Conj::No, out);
            }
        }
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
// Shift-contract rules: SELECTION (C2) and ORDER (C3)
//
// These are two different rules and no implementation may conflate them
// (PRD §5.2).  They are separate, separately named functions precisely so the
// two are not expressible as one tangled comparator: `select_nearest_to_shift`
// decides WHICH eigenvalues come back, `order_by_abs_lambda` decides in WHAT
// ORDER they are presented.  Conflating them turns a 300 Hz band inspection
// into the mode table 301, 298, 310, 295 instead of 295, 298, 301, 310.
//
// Both sorts are STABLE and keyed via `total_cmp`, so equidistant eigenvalues
// resolve deterministically from gevd's own index order — the crate's
// determinism suite depends on it.
// ---------------------------------------------------------------------------

/// **C2 — Selection.** Reorder `pairs` so its first `n_take` entries are the
/// eigenvalues nearest σ, and return that `n_take`.
///
/// The tail beyond `n_take` is left in place (still sorted by |λ − σ|) rather
/// than dropped: it is the *unselected* set, which the C5 provenance
/// computation needs.
fn select_nearest_to_shift(pairs: &mut [(f64, usize)], sigma: f64, n_modes: usize) -> usize {
    pairs.sort_by(|a, b| (a.0 - sigma).abs().total_cmp(&(b.0 - sigma).abs()));
    pairs.len().min(n_modes)
}

/// **C3 — Order.** Sort a selected set ascending by |λ|.
///
/// Absolute, not signed, which is the pre-PRD convention preserved unchanged:
/// with negative eigenvalues present λ=−2 still sorts before λ=+3.  Shared by
/// both implementations (SPOT) so the Lanczos path's presentation order cannot
/// drift from the dense path's.
fn order_by_abs_lambda(pairs: &mut [(f64, usize)]) {
    pairs.sort_by(|a, b| a.0.abs().total_cmp(&b.0.abs()));
}

/// **C5 — Provenance.** Whether some eigenvalue of the pencil lies STRICTLY
/// between zero and σ and is absent from the selected set.
///
/// Discrimination is on the `usize` source index, never on float equality of λ:
/// a pencil with a repeated eigenvalue would otherwise report one of the two
/// copies as "skipped" while its twin was returned.
///
/// Both signs of σ are covered, so a negative shift on the reversed-load
/// buckling side is handled by the same rule rather than a second one.  That is
/// a statement about the sign of **σ**, not about the sign of λ: an eigenvalue
/// on the far side of zero from σ is outside the interval by construction and
/// is never reported here, even when it is the smallest by `|λ|` and absent from
/// the selected set.  See the known-gap section on
/// [`EigenSolverResult::shift_skipped_modes`] — this function implements the PRD
/// §6 wording verbatim, and closing the gap is a PRD amendment, not a local fix.
///
/// At σ=0 the open interval is empty, so this is unconditionally `false` with no
/// branch — C1 preserved, and that `false` is ESTABLISHED rather than assumed.
fn any_eigenvalue_skipped_between_zero_and_shift(
    all_pairs: &[(f64, usize)],
    selected: &[(f64, usize)],
    sigma: f64,
) -> bool {
    all_pairs.iter().any(|&(lam, src_col)| {
        let between = (0.0 < lam && lam < sigma) || (sigma < lam && lam < 0.0);
        between && !selected.iter().any(|&(_, sel_col)| sel_col == src_col)
    })
}

/// **C5 — Provenance, conservative form.** The answer a path that did not
/// compute a spectrum it can count must give.
///
/// C5 permits `false` only when ESTABLISHED, never assumed, so any path without
/// the evidence to establish it reports `true` at σ≠0.  At σ=0 `false` IS
/// established with no evidence needed: the open interval strictly between 0 and
/// 0 is empty, so no eigenvalue can lie in it.
///
/// Exposed (and `pub`) rather than inlined because two crates need the identical
/// rule — the Lanczos path here and `reify-eval`'s degenerate
/// `singular_k_over_ceiling` early return, which returns no spectrum at all.
/// Writing it twice across a crate boundary is how the two drift when #7259
/// refines the Lanczos discriminator to a real one; this is the single place
/// that changes.
#[inline]
pub fn conservative_shift_provenance(sigma: f64) -> bool {
    sigma != 0.0
}

// ---------------------------------------------------------------------------
// Dense path
// ---------------------------------------------------------------------------

/// Solve the generalized symmetric eigenproblem `K φ = λ B φ` via dense QZ.
///
/// Densifies K and B, calls `faer::linalg::gevd::gevd_real`, recovers
/// `λ_i = S_re[i] / beta[i]` (skipping near-zero or infinite beta), then applies
/// the shift contract's two rules in order: selects the `n_modes` eigenvalues
/// nearest `opts.sigma` (C2) and presents them ascending by `|λ|` (C3).
///
/// Because `gevd_real` yields the ENTIRE spectrum, this path's
/// `shift_skipped_modes` answer is EXACT — it compares source indices between
/// the full spectrum and the selected set — whereas the shift-invert path can
/// only report a conservative boolean from its Cholesky/LU discriminator
/// (PRD §5.4 precision limit).
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

    // C2 then C3, in that order and never fused (see the helper block above).
    //
    // C1 (σ=0 is the identity) needs NO `if sigma == 0.0` branch: `λ − 0.0 == λ`
    // bit-exactly in IEEE-754 for every finite λ, so at σ=0 the selection sort
    // IS the pre-PRD `|λ|` sort, the prefix is the same slice, and a STABLE
    // re-sort of an already-|λ|-ascending prefix is a no-op.  Same faer calls in
    // the same order, so the buckling and modal goldens pass bit-for-bit.
    let n_take = select_nearest_to_shift(&mut pairs, opts.sigma, opts.n_modes);
    // C5 is EXACT here, so compute it from the whole vector and its selected
    // prefix while both are still in hand — nothing is re-derived later.
    let shift_skipped_modes =
        any_eigenvalue_skipped_between_zero_and_shift(&pairs, &pairs[..n_take], opts.sigma);
    order_by_abs_lambda(&mut pairs[..n_take]);
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
        shift: opts.sigma,
        shift_skipped_modes,
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
/// Solves `K φ = λ M φ` using shift-invert Lanczos.  Finds the smallest |λ| by
/// maximizing |μ| = 1/|λ| in the Krylov subspace of `K⁻¹ · M`.
///
/// **Shift contract status:** this path does not yet honor `opts.sigma` — the
/// `K − σB` assembly, the Cholesky-then-LU dispatch and the C4 back-shift are
/// task #7259.  It satisfies C1 and C3 today, and reports C5 conservatively (it
/// cannot ESTABLISH that nothing was skipped, and C5 forbids assuming it).
/// Until then it solves the UNSHIFTED pencil whatever `opts.sigma` says, and
/// **says so**: it reports `shift: 0.0` — the σ it actually used — rather than
/// echoing the request, so `result.shift == opts.sigma` is the caller's test for
/// whether the shift was honored.  See the module-level "Shift contract (C1–C6)"
/// section.
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

    // The σ this solve ACTUALLY uses, which is what `EigenSolverResult::shift`
    // is documented to report.  #7259 lands the `K − σB` assembly and the
    // Cholesky-then-LU dispatch; until then this path solves the UNSHIFTED
    // pencil whatever `opts.sigma` says, so it reports 0.0 rather than echoing
    // the request.  Echoing σ=0.6 over a bottom-of-the-spectrum answer would be
    // a correct-looking provenance field describing a solve that never happened
    // — the silent-substitution class this contract exists to close — and would
    // also make ε (#7262) refuse a result whose first mode is in fact present.
    // A caller that needs σ honored detects the gap with
    // `result.shift == opts.sigma`; #7259 replaces this with `opts.sigma`, and
    // when it does it must also add the `opts.sigma.is_finite()` guard this
    // function's option-contract asserts above deliberately omit — σ is inert
    // here precisely because it is never read.
    let shift_used = 0.0_f64;

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

    // C3 via the shared helper (SPOT).  SELECTION is deliberately NOT re-run
    // here: this path's converged set is the output of the UNSHIFTED operator,
    // so re-selecting it by |λ − σ| would be wrong until #7259 makes the
    // operator itself honor σ.
    order_by_abs_lambda(&mut pairs);

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
        shift: shift_used,
        // Shared with `reify-eval`'s degenerate early return (SPOT): a path that
        // cannot count what it skipped may not assume `false`.  Here `false` IS
        // established — the solve ran at σ=0 and the open interval strictly
        // between 0 and 0 is empty — and it stays correct for the ε (#7262)
        // consumer, which keys a refusal on this flag: the bottom of the
        // spectrum genuinely does contain the first mode.  #7259 passes
        // `opts.sigma` here once the operator honors it.
        shift_skipped_modes: conservative_shift_provenance(shift_used),
    }
}

// ---------------------------------------------------------------------------
// Sparse shift-invert wrapper (with dense fallback)
// ---------------------------------------------------------------------------

/// Solve `K φ = λ B φ` via shift-invert Lanczos.
///
/// **Shift contract status:** inherits [`lanczos_shift_invert`]'s — `opts.sigma`
/// is not yet honored on the Lanczos branch (#7259).  The dense fallback below
/// DOES honor it, so **which σ this entry point actually applies depends on the
/// problem dimension** until #7259 lands: n ≤ 64 routes dense and honors σ, a
/// larger problem routes Lanczos and solves at σ=0.  That divergence is not
/// silent — the returned [`EigenSolverResult::shift`] reports the σ actually
/// used, so a caller that needs the shift honored tests
/// `result.shift == opts.sigma` and gets a definite answer either way.
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
/// - K is not SPD (numeric Cholesky failure → panic with descriptive message,
///   matching Task-2544 panic-on-contract convention)
/// - The sparse Cholesky fails for a NON-numeric reason (`LltError::Generic`:
///   `OutOfMemory` / `IndexOverflow`). That panic is raised inside
///   [`try_solve_eigen_shift_invert`] and names the resource failure, so it is
///   never mistaken for the "K must be SPD" message below.
/// - See also [`check_eigen_options_and_shapes`]
///
/// A caller that can legitimately be handed a singular `K` — an
/// under-constrained modal model, say — should use
/// [`try_solve_eigen_shift_invert`] instead of catching this panic.
pub fn solve_eigen_shift_invert(
    k: &SparseRowMat<usize, f64>,
    b: &SparseRowMat<usize, f64>,
    opts: EigenSolverOptions,
) -> EigenSolverResult {
    try_solve_eigen_shift_invert(k, b, opts)
        .expect("eigensolve: K must be SPD; sp_cholesky failed — check that BCs have been applied")
}

/// Non-panicking sibling of [`solve_eigen_shift_invert`]: returns `None` when
/// the up-front sparse Cholesky of `K` fails.
///
/// `None` means EXACTLY ONE thing — `K` is not SPD (`sp_cholesky` returned
/// `LltError::Numeric`, i.e. a non-positive pivot). Every other precondition is
/// still a hard contract and still panics: the option/shape preconditions via
/// [`check_eigen_options_and_shapes`] (bad `n_modes`, shape mismatch, …), and a
/// `LltError::Generic` factorization failure (`FaerError::OutOfMemory` /
/// `IndexOverflow`) via an explicit panic at the match. `None` is therefore never
/// ambiguous between "singular K", "caller bug" and "out of memory", which is
/// what makes it safe for a caller to treat `None` as the domain fact "this
/// model is under-constrained" rather than as a generic failure. That
/// enforcement matters most on the large-mesh path: mapping an allocation
/// failure to `None` would surface it as
/// `W_ModalRigidBodyMode: K_free is singular (the model is under-constrained)`.
///
/// The factorization is performed exactly ONCE: on `Some` the very same `llt`
/// feeds [`SparseStiffnessOp`], so the healthy path pays no extra cost relative
/// to calling [`solve_eigen_shift_invert`] directly. (This is why the shape
/// here is `try_` + delegate rather than "probe with a throwaway `sp_cholesky`,
/// then call the panicking entry point", which would factor K twice on every
/// well-posed solve.)
///
/// # Panics
///
/// - See [`check_eigen_options_and_shapes`].
/// - The sparse Cholesky fails with `LltError::Generic` (`OutOfMemory` /
///   `IndexOverflow`) — a resource/index failure, not a property of the model.
///
/// A non-SPD `K` does NOT panic here; it is the `None` return.
pub fn try_solve_eigen_shift_invert(
    k: &SparseRowMat<usize, f64>,
    b: &SparseRowMat<usize, f64>,
    opts: EigenSolverOptions,
) -> Option<EigenSolverResult> {
    check_eigen_options_and_shapes(k, b, &opts);
    let n = k.nrows();

    // Factor K via sparse Cholesky. A NUMERIC failure here means K is not SPD —
    // the one condition this entry point reports rather than panics on.
    //
    // The error is MATCHED rather than `.ok()?`'d so that `None` really does
    // mean only that. faer's sparse `LltError` also carries a `Generic` arm
    // (`FaerError::OutOfMemory` / `IndexOverflow`), and mapping those to `None`
    // would let a resource failure factorizing a large `K_free` surface to the
    // user as `W_ModalRigidBodyMode: K_free is singular (the model is
    // under-constrained)` — a confidently wrong diagnosis of an allocation
    // problem, on the exact large-mesh path `DENSE_FALLBACK_MAX_DIM` exists to
    // serve. A `Generic` failure is not a domain fact about the model, so it
    // keeps the panicking contract.
    let llt = match k.sp_cholesky(Side::Lower) {
        Ok(llt) => llt,
        // K is not SPD (a non-positive pivot) — the documented `None`.
        Err(SparseLltError::Numeric(_)) => return None,
        Err(e @ SparseLltError::Generic(_)) => panic!(
            "eigensolve: sparse Cholesky of K failed for a non-numeric reason \
             ({e:?}) — this is a resource/index failure (allocation or index \
             overflow), NOT an under-constrained model; do not report it as a \
             rigid-body mode"
        ),
    };

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
        return Some(solve_eigen_dense(k, b, opts));
    }

    // Delegate to the generic Lanczos core via zero-cost adapter pair.
    // The chained matvec+backsolve through the adapters is byte-equivalent to
    // the former ShiftInvertOp composition (same faer calls in same order),
    // so buckling goldens pass bit-for-bit.
    let k_op = SparseStiffnessOp {
        factor: SparseFactorRef::Cholesky(&llt),
        n,
    };
    let m_op = SparseMetricOp { m: b.as_ref() };
    Some(lanczos_shift_invert(&k_op, &m_op, opts))
}
