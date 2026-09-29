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
//! - [`solve_eigen_shift_invert`] — shift-invert Lanczos: at σ=0 a sparse
//!   Cholesky of `K`, at σ≠0 a `K − σB` assembly factored Cholesky-then-LU,
//!   driving `faer::matrix_free::eigen::partial_self_adjoint_eigen`; falls back
//!   to dense when the Krylov window would exceed the problem dimension.
//! - [`try_solve_eigen_shift_invert`] — the same solve, reporting the two DOMAIN
//!   failures as a typed [`ShiftInvertFailure`] instead of panicking: `K` not
//!   SPD (task 6663, for callers that can legitimately be handed an
//!   under-constrained system) and a σ landing on an eigenvalue (C6). A
//!   resource failure (out of memory / index overflow) still panics and so can
//!   never arrive disguised as either.
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
//! | C1 | implemented | implemented, and STRUCTURAL — σ=0 factors `K` itself |
//! | C2 | implemented | implemented (same helper) |
//! | C3 | implemented (shared helper) | implemented (same helper) |
//! | C4 | n/a — no shift is ever applied to invert | implemented (`λ = σ + 1/μ`) |
//! | C5 | implemented, EXACT | implemented, conservative BOOLEAN (Sylvester) |
//! | C6 | vacuous — no `K − σB` is ever formed | implemented, two-part (PRD §5.3) |
//!
//! Both paths now honor σ, so [`solve_eigen_shift_invert`] applies the requested
//! shift at every problem size. [`EigenSolverResult::shift`] still reports the σ
//! a solve actually USED rather than the one it was asked for — that is a
//! standing property of the field, not scaffolding for a gap that has closed.
//!
//! The dense path honors σ as a SORT-KEY CHANGE AND NOTHING ELSE: `gevd_real`
//! computes the entire spectrum, so no factorization is formed, `K − σB` never
//! exists, and no new failure mode is introduced. That is why it landed first
//! and the Lanczos implementation is held to it: the acceptance criterion is
//! `tests/eigensolve_shift_contract.rs`, whose σ≠0 arms instantiate the same
//! harness functions both paths are measured by.
//!
//! The two paths differ in exactly two places, both forced and both documented
//! where they live: C5 is EXACT on dense and a conservative boolean on Lanczos
//! (see [`shift_provenance_from_factorization`]), and C6 is vacuous on dense
//! because no `K − σB` is formed there at all.
//!
//! # Convergence quality versus σ — MEASURED, not predicted
//!
//! faer's `partial_self_adjoint_eigen` orthogonalizes in the EUCLIDEAN inner
//! product, while `(K − σB)⁻¹B` is self-adjoint in the `(K − σB)` form — which
//! stops being an inner product at all once that matrix is indefinite, i.e.
//! exactly when σ rises above some mode.  Whether that costs accuracy is an
//! empirical question, so it was measured rather than argued.
//!
//! Measured 2026-09-16 on fixture C (`K` = tridiag(−1,2,−1) 80×80, `B` = I,
//! closed form `λ_k = 2(1 − cos(kπ/81))`), n_modes=2, tol=1e-10, debug:
//!
//! | σ | n_converged | converged | max \|λ − λ_k\| | max residual |
//! |---|---|---|---|---|
//! | λ₁/2 = 7.52e-4 | 2 | true | 5.29e-17 | 2.39e-13 |
//! | mid(λ₉, λ₁₀) = 0.1346 | 2 | true | 2.08e-16 | 4.26e-15 |
//! | mid(λ₃₉, λ₄₀) = 1.9225 | 2 | true | 2.22e-16 | 2.88e-16 |
//! | mid(λ₅₉, λ₆₀) = 3.3438 | 2 | true | 4.44e-16 | 2.31e-16 |
//! | mid(λ₇₄, λ₇₅) = 3.9364 | 2 | true | 0.00 | 1.54e-16 |
//!
//! (residual = `‖Kφ − λBφ‖ / ‖Kφ‖`.)  **No degradation as σ grows**, including
//! at the σ where `K − σB` has 74 negative eigenvalues.  So no
//! `converged: false` response is called for, and certainly no Krylov rewrite —
//! a B-orthogonal or `(K − σB)`-orthogonal Lanczos is out of scope per PRD §8.
//!
//! **What this measurement does NOT cover**, stated so the table is not read as
//! more than it is: `B` = I makes `(K − σB)⁻¹B = (K − σI)⁻¹`, which IS
//! Euclidean-self-adjoint however indefinite it becomes.  Fixture C therefore
//! cannot exercise the hazard in its general form.  A companion probe with
//! `B` = diag(1 + i/80) (SPD, non-identity) was also measured and behaves quite
//! differently — the residual is 3.58e-1 there — but that is a PRE-EXISTING
//! property of this path, not a σ effect: it is WORST at σ=0, it improves
//! monotonically to 9.3e-3 as σ grows, and the same 3.58e-1 is measured at the
//! pre-PRD base commit.  Recorded as esc-7259-1 and filed as a follow-up task;
//! it is out of scope here because the affected path is σ=0, which this module
//! is required to leave byte-for-byte unchanged.
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
use faer::sparse::linalg::LuError as SparseLuError;
use faer::sparse::{SparseColMat, SparseRowMat, SparseRowMatRef};
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
    /// and the selected set — while the **shift-invert path** reads its answer
    /// off which factorization succeeded, by Sylvester's law of inertia, giving
    /// a conservative boolean (an exact count would need an inertia-revealing
    /// LDL^T that faer's sparse LU does not expose).  The two predicates are
    /// therefore not the same predicate — absence-based versus position-based —
    /// and the shift-invert path OVER-reports in one configuration, which is the
    /// direction C5 permits; see [`shift_provenance_from_factorization`] for the
    /// full statement.  At σ=0 every path reports `false`, and that `false` is
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
    /// requested σ would report the one it did solve at.  Both paths honor σ
    /// today, so the two now agree on every successful solve — but the field
    /// keeps its meaning rather than becoming a copy, because a caller must
    /// never have to infer from the problem dimension which σ was applied.
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

// ---------------------------------------------------------------------------
// The shifted pencil K − σB
// ---------------------------------------------------------------------------

/// Assemble `K − σB` over the UNION of the two sparsity patterns.
///
/// Returned COLUMN-major, deliberately.  `faer::sparse::ops::binary_op` is
/// CSC-only, and `sp_cholesky` / `sp_lu` both exist on the column-major type
/// (faer-0.24 `sparse/solvers.rs`), so factoring this result directly also skips
/// the `to_col_major()` copy the row-major forms make internally.
///
/// # The result carries EXPLICIT ZEROS, and that is why σ=0 is special-cased
///
/// `binary_op` walks the STRUCTURAL union: it stores an entry wherever EITHER
/// operand has one, and applies `f` with `None` for the absent side.  So every
/// entry of B that is off K's pattern is stored here even when the arithmetic
/// cancels it to zero.
///
/// At σ=0 that matters.  `K − 0·B` is numerically equal to K entry by entry, but
/// it is NOT the same sparse matrix: the off-pattern entries of B arrive as
/// explicit zeros, which change the symbolic factorization — different fill-in,
/// a different elimination tree, therefore a different summation order and
/// different rounding.  The answer would be *close*, which is exactly the
/// hazard: it would drift every pinned σ=0 golden by an amount no tolerance
/// catches.  `try_solve_eigen_shift_invert` therefore routes σ=0 to
/// `k.sp_cholesky(Side::Lower)` on the row-major K verbatim and never calls this
/// function, and `sigma_zero_factors_k_itself_not_k_minus_zero_b` in
/// `tests/eigensolve_shift_contract.rs` is the executable form of that rule.
///
/// # Panics
///
/// If either `to_col_major()` or `binary_op` returns `FaerError`
/// (`OutOfMemory` / `IndexOverflow`).  That mirrors the `SparseLltError::Generic`
/// discipline elsewhere in this module: an allocation or index-overflow failure
/// is not a domain fact about the model, so it must never arrive disguised as
/// one — a resource failure reported as a singular shift would send an author
/// to move σ for a problem that is entirely about memory.
fn shifted_pencil(
    k: &SparseRowMat<usize, f64>,
    b: &SparseRowMat<usize, f64>,
    sigma: f64,
) -> SparseColMat<usize, f64> {
    let k_csc = k
        .to_col_major()
        .unwrap_or_else(|e| panic!("{}", assembly_resource_failure("K", e)));
    let b_csc = b
        .to_col_major()
        .unwrap_or_else(|e| panic!("{}", assembly_resource_failure("B", e)));

    faer::sparse::ops::binary_op(k_csc.as_ref(), b_csc.as_ref(), |k_ij, b_ij| {
        k_ij.copied().unwrap_or(0.0) - sigma * b_ij.copied().unwrap_or(0.0)
    })
    .unwrap_or_else(|e| panic!("{}", assembly_resource_failure("K − σB", e)))
}

/// The panic message for a `FaerError` raised while assembling `K − σB`.
///
/// One template rather than three literals, so the three raise sites cannot
/// drift apart in wording (SPOT).
fn assembly_resource_failure(what: &str, e: faer::sparse::FaerError) -> String {
    format!(
        "eigensolve: assembling the shifted pencil failed while building {what} ({e:?}) \
         — this is a resource/index failure (allocation or index overflow), NOT a \
         property of the pencil; do not report it as a singular shift"
    )
}

/// A DOMAIN failure of the shift-invert path — a fact about the (K, B, σ) the
/// caller handed in, never about the machine it ran on.
///
/// Both arms are things an author can act on by changing the model or the
/// shift, which is precisely why they are typed values rather than panics.  A
/// resource failure (`FaerError::OutOfMemory` / `IndexOverflow`) is NOT in this
/// enum and never will be: it is not a fact about the model, so it keeps the
/// panicking contract and cannot arrive disguised as one of these.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShiftInvertFailure {
    /// `K` is not symmetric positive definite — its Cholesky hit a non-positive
    /// pivot.  The model is under-constrained (a DOF no element and no
    /// Dirichlet BC restrains), or the assembled `K` is otherwise singular.
    ///
    /// NOT reported at σ≠0: see [`try_solve_eigen_shift_invert`] for why that
    /// detection limit is deliberate and what a caller using this as an
    /// under-constrained-model detector must do about it.
    KNotSpd,
    /// `K − σB` is singular, or numerically indistinguishable from singular, at
    /// this shift: σ sits on (or within the pencil's own resolution floor of)
    /// an eigenvalue, so shift-invert has no operator to apply.  The remedy is
    /// to MOVE σ — which is why σ is carried here rather than left for the
    /// caller to re-derive from its own options (contract clause C6).
    ///
    /// # Distinct fault, distinct remedy — the canonical statement
    ///
    /// This and [`Self::KNotSpd`] must never be merged, and every consumer that
    /// carries the distinction upward cites THIS paragraph rather than restating
    /// it.  `KNotSpd` says "the model is under-constrained, add supports";  this
    /// one says "`K − σB` is singular AT THE REQUESTED SHIFT, move σ" — on a
    /// model whose supports may be perfectly fine.  Collapsing the two sends an
    /// author to check boundary conditions for a problem that is entirely about
    /// where σ was put, which is a confidently wrong diagnosis rather than a
    /// merely unhelpful one.
    ///
    /// # Honesty limit
    ///
    /// [`shift_is_numerically_singular`] also answers `true` for an EMPTY or
    /// non-finite spectrum, so this arm can be reached when a σ≠0 solve simply
    /// converged nothing.  The conflation is real, pinned by
    /// `empty_spectrum_at_a_healthy_shift_is_reported_as_a_singular_shift`, and
    /// deliberately not split here — the split is tracked as #7617.
    ShiftAtEigenvalue { sigma: f64 },
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
/// rule for the identical reason — they have NO SPECTRUM AT ALL to reason about.
/// Its two callers are [`lanczos_shift_invert`], which is handed an opaque
/// pre-built factorization and cannot inspect it, and `reify-eval`'s degenerate
/// `singular_k_over_ceiling` early return, which computes nothing. Writing the
/// rule twice across a crate boundary is how the two drift.
///
/// It is NOT the answer the sparse shift-invert path gives. That path DOES have
/// evidence — which of Cholesky / LU factored `K − σB` — and
/// [`try_solve_eigen_shift_invert`] uses it to establish `false` where this
/// helper could only assume `true`. The refinement is not expressible as a
/// function of σ alone, which is why it lives at the dispatch rather than
/// replacing this helper.
#[inline]
pub fn conservative_shift_provenance(sigma: f64) -> bool {
    sigma != 0.0
}

/// **C5 — Provenance, from the Sylvester evidence the §5.1 dispatch produced.**
///
/// Cholesky of a symmetric matrix succeeds iff that matrix is positive definite.
/// `K − σB` is positive definite iff the pencil has no eigenvalue between zero
/// and σ — so which factorization won IS the answer, computed for free by a
/// dispatch that had to run anyway.
///
/// For buckling's indefinite `B = −K_g` this correctly means no mode has been
/// passed in EITHER direction, because definiteness is a two-sided statement;
/// no second rule for negative σ is needed.
///
/// # Precision limit, fixed by the PRD and not negotiable here
///
/// This is a BOOLEAN and can never be a count. An exact count of the modes
/// below σ needs the INERTIA of `K − σB`, which requires an inertia-revealing
/// `LDL^T`; faer's sparse LU does not expose one. The dense path counts exactly.
/// So a consumer formatting a diagnostic must use ONE message template with an
/// OPTIONAL count, never two templates that drift apart.
///
/// # This predicate is POSITION-based; the dense one is ABSENCE-based
///
/// [`any_eigenvalue_skipped_between_zero_and_shift`] asks whether an eigenvalue
/// in the interval is ABSENT from the returned set. This one can only ask
/// whether an eigenvalue is IN the interval at all — it has no spectrum to check
/// absence against.
///
/// Cholesky SUCCESS implies nothing is in the interval, hence nothing in it is
/// absent, so `false` here is always sound and always established. Cholesky
/// FAILURE implies only that something is in the interval; it may still have
/// been RETURNED, and in that configuration the dense path answers `false` while
/// this one answers `true`. **Lanczos over-reports, and over-reporting is the
/// direction C5 permits** — `false` only when established. It is not a defect
/// and must not be "fixed" by weakening either side.
///
/// `cholesky_succeeded` is about the matrix the dispatch actually factored —
/// `K − σB`, which at σ=0 is `K` itself.
#[inline]
fn shift_provenance_from_factorization(sigma: f64, cholesky_succeeded: bool) -> bool {
    // σ=0: established `false` with no evidence needed — the open interval
    // strictly between 0 and 0 is empty.
    sigma != 0.0 && !cholesky_succeeded
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

/// Shift-invert Lanczos eigensolver over arbitrary self-adjoint operator pairs.
///
/// Solves `K φ = λ M φ` using shift-invert Lanczos.  Finds the λ nearest σ by
/// maximizing |μ| = 1/|λ − σ| in the Krylov subspace of `(K − σM)⁻¹ · M`.
///
/// # `opts.sigma` is a DESCRIPTION of `k_op`, not an instruction to it
///
/// This function never forms `K − σB`; it is handed an already-built
/// factorization and applies it.  `opts.sigma` tells it which σ that
/// factorization embeds, so that it can back-shift (C4, `λ = σ + 1/μ`) and
/// select (C2, nearest `|λ − σ|`) correctly.
///
/// **The core cannot verify the claim.** A `k_op` factoring `K − 0.3·B` passed
/// with `opts.sigma = 0.7` produces a plausible, wrong spectrum, and nothing
/// here can detect it — the operator is opaque by design, which is what lets
/// matrix-free and lumped-diagonal callers use this path at all.  Keeping the
/// two consistent is the caller's obligation.
///
/// C6 (a singular or numerically-degenerate `K − σB` is a typed failure
/// carrying σ) therefore belongs to whoever BUILT the factorization, not here:
/// only that code has the matrices needed to size a resolution floor and decide
/// the question.  [`try_solve_eigen_shift_invert`] is where it lives for the
/// sparse path.
///
/// This is the generic core — it operates over any [`StiffnessOp`] /
/// [`MetricOp`] pair without knowledge of the underlying representation
/// (sparse CSR, matrix-free, lumped diagonal, etc.).  **No dense fallback**:
/// the caller is responsible for small-problem dispatch.  For the common
/// sparse case with automatic dense fallback use [`solve_eigen_shift_invert`].
///
/// # Parameters
///
/// - `k_op`: a pre-factored inverse of whatever matrix the caller means to
///   invert (e.g. sparse Cholesky or LU via [`SparseStiffnessOp`])
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
    assert!(
        opts.sigma.is_finite(),
        "EigenSolverOptions.sigma = {} must be finite",
        opts.sigma,
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
    // is documented to report.  It is `opts.sigma` because `k_op` is a
    // factorization of `K − σB` for THAT σ — see this function's rustdoc on why
    // the core cannot verify that and why C6 belongs to whoever built it.
    let shift_used = opts.sigma;

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

    // C4 — back-shift μ → λ = σ + 1/μ for the converged modes only.  (At σ=0
    // this is the pre-PRD `λ = 1/μ`, bit-exactly: `0.0 + x == x` for every
    // finite x.)
    //
    // The two filters are UNCHANGED and both are about spurious pencil modes,
    // not about a singular shift: μ≈0 means λ→∞, an eigenvector of B's null
    // space rather than of the pencil.  A σ sitting ON an eigenvalue is the
    // opposite limit — |μ|→∞ — and is caught by the §5.3 guard in
    // `try_solve_eigen_shift_invert`, which has the matrices needed to size it.
    let mut pairs: Vec<(f64, usize)> = (0..n_conv)
        .filter_map(|i| {
            let mu = eigvals_mu[i];
            if mu.abs() < f64::MIN_POSITIVE {
                return None;
            }
            let lambda = shift_used + 1.0 / mu;
            if lambda.is_finite() { Some((lambda, i)) } else { None }
        })
        .collect();

    // C2 then C3, in that order, via the shared helpers (SPOT).  SELECTION
    // must run BEFORE presentation: truncating an |λ|-ordered list to n_modes
    // would select by |λ| instead of |λ − σ| and silently violate C2 whenever
    // faer converges more pairs than were asked for.
    //
    // At σ=0 this is structurally a no-op: |λ − 0.0| == |λ| bit-exactly, so
    // `select_nearest_to_shift` produces the same order the old
    // `order_by_abs_lambda` did, and a STABLE re-sort of an already-ascending
    // prefix cannot move anything.  C1 is preserved.
    let n_take = select_nearest_to_shift(&mut pairs, shift_used, opts.n_modes);
    order_by_abs_lambda(&mut pairs[..n_take]);
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
        // cannot count what it skipped may not assume `false`.  This core is
        // handed an OPAQUE factorization, so it has no evidence to establish
        // `false` at σ≠0 and C5 forbids it assuming one.
        //
        // `try_solve_eigen_shift_invert` BUILT the factorization and therefore
        // does have the evidence; it overrides this with
        // `shift_provenance_from_factorization` via `with_provenance`.  A caller
        // driving this core directly keeps the conservative answer, which is the
        // correct one for what it knows.
        shift_skipped_modes: conservative_shift_provenance(shift_used),
    }
}

// ---------------------------------------------------------------------------
// Sparse shift-invert wrapper (with dense fallback)
// ---------------------------------------------------------------------------

/// Solve `K φ = λ B φ` via shift-invert Lanczos.
///
/// **Shift contract status:** `opts.sigma` is honored on BOTH branches.  At σ=0
/// the sparse branch factors `K` itself (C1 — see
/// [`try_solve_eigen_shift_invert`]); at σ≠0 it assembles `K − σB`, factors it
/// Cholesky-then-LU, and back-shifts `λ = σ + 1/μ`.  The dense fallback honors σ
/// as a selection key over the full computed spectrum.
///
/// Delegates to [`try_solve_eigen_shift_invert`], which owns the dispatch, the
/// C5 provenance and the C6 singular-shift guard.  **This entry point is a
/// convenience over that one and nothing more** — it decides neither the
/// dispatch nor the guard, and must not become a second place where either is
/// decided.  All it adds is turning the two typed domain failures into panics.
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
/// Three causes, kept DISTINCT because their remedies are distinct — a message
/// that conflated any two would send an author to fix the wrong thing:
///
/// - **`K` is not SPD** (numeric Cholesky failure). Message contains
///   `"K must be SPD"`; the remedy is to apply boundary conditions. Matches the
///   Task-2544 panic-on-contract convention.
/// - **`K − σB` is singular at this σ** — σ sits on, or within the pencil's own
///   resolution floor of, an eigenvalue. Names the offending σ and says to move
///   it. This message deliberately shares NO wording with the one above.
/// - **A `Generic` resource failure** from the `K − σB` assembly or from either
///   factorization (`OutOfMemory` / `IndexOverflow`). Raised inside
///   [`try_solve_eigen_shift_invert`] and names the resource failure, so it is
///   never mistaken for either domain fault.
/// - See also [`check_eigen_options_and_shapes`] for the option/shape guards.
///
/// A caller that can legitimately be handed either DOMAIN failure — a singular
/// `K` from an under-constrained modal model, or a σ it cannot place safely in
/// advance — should use [`try_solve_eigen_shift_invert`] instead of catching
/// these panics: it reports both as typed values that can be told apart.
pub fn solve_eigen_shift_invert(
    k: &SparseRowMat<usize, f64>,
    b: &SparseRowMat<usize, f64>,
    opts: EigenSolverOptions,
) -> EigenSolverResult {
    match try_solve_eigen_shift_invert(k, b, opts) {
        Ok(result) => result,
        Err(ShiftInvertFailure::KNotSpd) => panic!(
            "eigensolve: K must be SPD; sp_cholesky failed — check that BCs have been applied"
        ),
        // Shares NO wording with the message above, deliberately: this is a
        // fault in where σ was placed, and reporting it in the vocabulary of a
        // non-SPD K would send an author to check boundary conditions for a
        // problem that is entirely about the shift.
        Err(ShiftInvertFailure::ShiftAtEigenvalue { sigma }) => panic!(
            "eigensolve: the shift sigma = {sigma} lands on an eigenvalue of the pencil, \
             so K - sigma*B is singular and shift-invert has no operator to apply; move \
             the shift off that eigenvalue"
        ),
    }
}

/// Non-panicking sibling of [`solve_eigen_shift_invert`]: reports a DOMAIN
/// failure of the shift-invert path as a typed [`ShiftInvertFailure`] rather
/// than panicking.
///
/// `Err` means EXACTLY ONE CLASS OF THING — a fact about the (K, B, σ) handed
/// in, enumerated by [`ShiftInvertFailure`] and distinguishable arm by arm.
/// `Err(KNotSpd)` in particular means EXACTLY that `K` is not SPD
/// (`sp_cholesky` returned `LltError::Numeric`, i.e. a non-positive pivot).
/// Every other precondition is still a hard contract and still panics: the
/// option/shape preconditions via [`check_eigen_options_and_shapes`] (bad
/// `n_modes`, shape mismatch, …), and a `LltError::Generic` factorization
/// failure (`FaerError::OutOfMemory` / `IndexOverflow`) via an explicit panic at
/// the match. `Err(KNotSpd)` is therefore never ambiguous between "singular K",
/// "caller bug" and "out of memory", which is what makes it safe for a caller to
/// treat it as the domain fact "this model is under-constrained" rather than as
/// a generic failure. That enforcement matters most on the large-mesh path:
/// mapping an allocation failure into this channel would surface it as
/// `W_ModalRigidBodyMode: K_free is singular (the model is under-constrained)`.
///
/// Typing the channel rather than returning a bare `None` is what keeps the two
/// domain failures DISTINGUISHABLE; [`ShiftInvertFailure::ShiftAtEigenvalue`]
/// states why that distinction is load-bearing.
///
/// # `Err(KNotSpd)` is reported at σ=0 only — a DETECTION limit, not a claim
///
/// `K`'s own SPD-ness is measured only where `K` itself is factored, which is
/// the σ=0 branch. At σ≠0 the factored matrix is `K − σB`, and on a problem
/// small enough to route to [`solve_eigen_dense`] nothing is factored at all —
/// so the very same singular `K` yields `Err(KNotSpd)` at σ=0 and `Ok(dense
/// result)` at σ≠0.
///
/// The asymmetry is deliberate rather than hoisted away. `Ok` at σ≠0 is not
/// wrong: the dense QZ path tolerates a singular `K` and returns the real
/// spectrum, rigid-body modes and all, which is exactly what the caller asked
/// for. Hoisting the dense check above the σ split to make the two branches
/// agree would instead make the σ=0 branch STOP reporting a fault it can cheaply
/// measure, and would change the σ=0 code path that
/// `sigma_zero_factors_k_itself_not_k_minus_zero_b` pins. So the limit is
/// documented instead: a caller using this entry point as an
/// under-constrained-model DETECTOR (task 6663) must call it at σ=0, which is
/// what `reify-eval`'s `solve_generalized_eigen` does — it pre-screens the whole
/// dense regime before ever reaching here.
///
/// The factorization is performed exactly ONCE: on `Ok` the very same factor
/// feeds [`SparseStiffnessOp`], so the healthy path pays no extra cost relative
/// to calling [`solve_eigen_shift_invert`] directly. (This is why the shape
/// here is `try_` + delegate rather than "probe with a throwaway `sp_cholesky`,
/// then call the panicking entry point", which would factor K twice on every
/// well-posed solve.)
///
/// # Singular shifts (C6), detected in TWO parts because either alone is unsound
///
/// At σ≠0 this function forms `K − σB` and factors it, so it owns C6 — the
/// generic core cannot, having only an opaque factorization.  Detection is
/// PRD §5.3's two parts:
///
/// - **Part A** — `sp_lu` returns `LuError::SymbolicSingular`, i.e. STRUCTURAL
///   rank deficiency: no pivot exists anywhere in the pattern.
/// - **Part B** — the post-factorization guard,
///   [`shift_is_numerically_singular`], keyed on the λ-space resolution floor
///   [`pencil_lambda_resolution_floor`] derives from the pencil's own scale.
///
/// Part B is MANDATORY, not belt-and-braces. Part A cannot fire whenever
/// `K − σB` keeps a full diagonal, which is the ordinary case: the symbolic
/// structure stays full-rank however close σ gets to an eigenvalue, partial
/// pivoting proceeds on a tiny-but-nonzero pivot, and the solve returns a
/// plausible spectrum with one entry pinned to σ and the rest quietly wrong.
/// The guard runs on the whole σ≠0 branch, including the arm where the shifted
/// Cholesky succeeded, so a σ numerically on a mode is caught whichever
/// factorization won.
///
/// **A singular shift is never silently repaired.** No perturbation of σ, no
/// re-solve at a nudged shift — see [`shift_is_numerically_singular`].
///
/// # Panics
///
/// - See [`check_eigen_options_and_shapes`].
/// - Either factorization fails with a `Generic` error (`OutOfMemory` /
///   `IndexOverflow`), as does the `K − σB` assembly — resource/index failures,
///   not properties of the model or the shift.
///
/// Neither a non-SPD `K` nor a singular shift panics here; both are `Err`.
pub fn try_solve_eigen_shift_invert(
    k: &SparseRowMat<usize, f64>,
    b: &SparseRowMat<usize, f64>,
    opts: EigenSolverOptions,
) -> Result<EigenSolverResult, ShiftInvertFailure> {
    check_eigen_options_and_shapes(k, b, &opts);
    let n = k.nrows();
    let m_op = SparseMetricOp { m: b.as_ref() };

    // ---- σ=0: TODAY'S EXACT PATH, and deliberately not one line more. -------
    //
    // `K − 0·B` is numerically equal to K but is NOT the same sparse matrix: an
    // assembly over the union of the two patterns stores B's off-pattern entries
    // as explicit zeros, which changes the Cholesky fill-in and therefore the
    // rounding.  Routing σ=0 through the shifted branch would move every pinned
    // σ=0 golden by an amount no tolerance catches.  `shifted_pencil`'s rustdoc
    // carries the full argument; `sigma_zero_factors_k_itself_not_k_minus_zero_b`
    // is the executable form.
    if opts.sigma == 0.0 {
        // Factor K via sparse Cholesky. A NUMERIC failure here means K is not SPD
        // — the one condition this entry point reports rather than panics on.
        //
        // The error is MATCHED rather than `.ok()?`'d so that `Err(KNotSpd)` really
        // does mean only that. faer's sparse `LltError` also carries a `Generic` arm
        // (`FaerError::OutOfMemory` / `IndexOverflow`), and mapping those into the
        // failure channel would let a resource failure factorizing a large `K_free`
        // surface to the user as `W_ModalRigidBodyMode: K_free is singular (the model
        // is under-constrained)` — a confidently wrong diagnosis of an allocation
        // problem, on the exact large-mesh path `DENSE_FALLBACK_MAX_DIM` exists to
        // serve. A `Generic` failure is not a domain fact about the model, so it
        // keeps the panicking contract.
        let llt = match k.sp_cholesky(Side::Lower) {
            Ok(llt) => llt,
            // K is not SPD (a non-positive pivot) — the documented `Err(KNotSpd)`.
            Err(SparseLltError::Numeric(_)) => return Err(ShiftInvertFailure::KNotSpd),
            Err(e @ SparseLltError::Generic(_)) => panic!(
                "eigensolve: sparse Cholesky of K failed for a non-numeric reason \
                 ({e:?}) — this is a resource/index failure (allocation or index \
                 overflow), NOT an under-constrained model; do not report it as a \
                 rigid-body mode"
            ),
        };

        if routes_to_dense_fallback(n, opts.n_modes) {
            // Problem too small for Lanczos; delegate to the direct dense solver.
            // The dense result already satisfies the EigenSolverResult contract
            // (converged=true, iterations=0, eigenvalues sorted ascending |λ|).
            return Ok(solve_eigen_dense(k, b, opts));
        }

        // Delegate to the generic Lanczos core via zero-cost adapter pair.
        // The chained matvec+backsolve through the adapters is byte-equivalent to
        // the former ShiftInvertOp composition (same faer calls in same order),
        // so buckling goldens pass bit-for-bit.
        let k_op = SparseStiffnessOp {
            factor: SparseFactorRef::Cholesky(&llt),
            n,
        };
        return Ok(with_provenance(
            lanczos_shift_invert(&k_op, &m_op, opts),
            0.0,
            true,
        ));
    }

    // ---- σ≠0: the shifted pencil. ------------------------------------------
    //
    // The dense check comes FIRST here, unlike the σ=0 branch above.  The dense
    // path honors σ as a sort-key change and forms no `K − σB` at all, so a
    // small problem must not acquire a factorization failure mode it does not
    // need — and there is no point assembling a pencil that is about to be
    // discarded.  (At σ=0 the order is the other way round because that IS the
    // pre-PRD order, and C1 is a claim about the code path, not just the
    // numbers.)
    if routes_to_dense_fallback(n, opts.n_modes) {
        return Ok(solve_eigen_dense(k, b, opts));
    }

    let shifted = shifted_pencil(k, b, opts.sigma);
    let sigma = opts.sigma;

    // PRD §5.1 dispatch: Cholesky FIRST, LU only on a numeric failure.
    //
    // The Cholesky attempt is not merely an optimization and must NOT be
    // collapsed into an unconditional LU: by Sylvester's law of inertia its
    // SUCCESS proves `K − σB` is positive definite, which is exactly the
    // statement that no eigenvalue of the pencil lies between zero and σ.  That
    // one bit is the C5 discriminator `with_provenance` reads off this dispatch.
    //
    // PRD §5.3 PART A lives here: the one singular-shift case faer reports
    // directly.
    let (result, cholesky_succeeded) = match shifted.sp_cholesky(Side::Lower) {
        Ok(llt) => {
            let k_op = SparseStiffnessOp {
                factor: SparseFactorRef::Cholesky(&llt),
                n,
            };
            (lanczos_shift_invert(&k_op, &m_op, opts), true)
        }
        // `K − σB` is indefinite — the expected case for a σ above some mode,
        // not an error. LU handles it.
        Err(SparseLltError::Numeric(_)) => match shifted.sp_lu() {
            Ok(lu) => {
                let k_op = SparseStiffnessOp {
                    factor: SparseFactorRef::Lu(&lu),
                    n,
                };
                (lanczos_shift_invert(&k_op, &m_op, opts), false)
            }
            // STRUCTURAL rank deficiency — no pivot exists anywhere in the
            // pattern, so `K − σB` is singular and shift-invert has no operator
            // to apply.  faer reports the elimination step at which the pivot
            // search failed (`index`); it is deliberately NOT carried in the
            // typed value, because C6's remedy is "move σ" and an internal
            // elimination index names nothing the caller can act on.  It is
            // recorded here rather than dropped silently.
            Err(SparseLuError::SymbolicSingular { .. }) => {
                return Err(ShiftInvertFailure::ShiftAtEigenvalue { sigma });
            }
            Err(e @ SparseLuError::Generic(_)) => panic!(
                "eigensolve: sparse LU of K − σB failed for a non-numeric reason \
                 ({e:?}) — this is a resource/index failure (allocation or index \
                 overflow), NOT a singular shift; do not report it as one"
            ),
        },
        Err(e @ SparseLltError::Generic(_)) => panic!(
            "eigensolve: sparse Cholesky of K − σB failed for a non-numeric reason \
             ({e:?}) — this is a resource/index failure (allocation or index \
             overflow), NOT a property of the pencil; do not report it as a \
             singular shift"
        ),
    };

    // PRD §5.3 PART B, on the WHOLE σ≠0 branch — including the arm where the
    // Cholesky succeeded, so a σ numerically on a mode is caught whichever
    // factorization won.
    if shift_is_numerically_singular(
        &result.eigenvalues,
        sigma,
        pencil_lambda_resolution_floor(&shifted, b),
    ) {
        return Err(ShiftInvertFailure::ShiftAtEigenvalue { sigma });
    }

    Ok(with_provenance(result, sigma, cholesky_succeeded))
}

/// The pencil's own λ-space resolution floor: the distance below which two
/// eigenvalues of `(K, B)` are not distinguishable in f64 by THIS
/// factorization.
///
/// # Derivation — check it, do not trust it
///
/// Sparse LU with partial pivoting is BACKWARD stable: the computed
/// factorization is the exact one of some `A + δA` with
/// `‖δA‖ ≲ n · ε · ‖A‖`, where `A = K − σB` and ε is `f64::EPSILON`.  A
/// perturbation δA of the pencil `(A, B)` moves an eigenvalue by at most
/// `|δλ| ≲ ‖δA‖ / ‖B‖`.  Composing the two:
///
/// ```text
/// λ_floor = n · ε · ‖K − σB‖_∞ / ‖B‖_∞
/// ```
///
/// Every quantity comes from the matrices in hand plus machine epsilon.  No
/// constant is imported from another solver, and none is tuned to a fixture.
///
/// # Why a residual test does NOT work here
///
/// The obvious alternative — back-substitute and measure `‖Ax − r‖` — is
/// useless for this question precisely BECAUSE LU is backward stable: the
/// computed `x` satisfies `(A + δA)x = r` with small δA even when `A` is
/// numerically singular, so the residual stays tiny either way.  The signal has
/// to be read in λ space, after the back-shift, where a numerically singular
/// `A` shows up as a recovered λ collapsing onto σ.
fn pencil_lambda_resolution_floor(
    shifted: &SparseColMat<usize, f64>,
    b: &SparseRowMat<usize, f64>,
) -> f64 {
    let shifted_ref = shifted.as_ref();
    let shifted_sym = shifted_ref.symbolic();
    let mut row_sums = vec![0.0_f64; shifted.nrows()];
    for j in 0..shifted.ncols() {
        let rows = shifted_sym.row_idx_of_col_raw(j);
        let vals = shifted_ref.val_of_col(j);
        for (&i, &v) in rows.iter().zip(vals.iter()) {
            row_sums[i] += v.abs();
        }
    }
    let norm_inf_shifted = row_sums.into_iter().fold(0.0_f64, f64::max);

    // `‖B‖_∞` is the DIVISOR, so a degenerate B has to be refused rather than
    // divided by. It is a CALLER bug — the generalized problem `Kφ = λBφ` is
    // not defined without a B — and left unguarded it fails silently in two
    // OPPOSITE directions: an all-zero B makes the floor `+∞`, so
    // `shift_is_numerically_singular` answers `true` for every σ≠0 and a healthy
    // solve is refused as a singular shift; and if `K − σB` is all zero too the
    // floor is `NaN`, every `<=` against it is false, and PART B of the C6 guard
    // silently disappears. Neither is visible from the outside, which is the
    // argument for a loud panic over a defensive default.
    //
    // Checked HERE rather than in `check_eigen_options_and_shapes` on purpose:
    // this is the only site that divides by it, and the σ=0 path — every pinned
    // golden — forms no floor at all, so a guard hoisted to the shared entry
    // check would newly refuse σ=0 callers over a quantity they never use.
    let b_ref = b.as_ref();
    let norm_inf_b = (0..b.nrows())
        .map(|i| b_ref.val_of_row(i).iter().map(|v| v.abs()).sum::<f64>())
        .fold(0.0_f64, f64::max);
    assert!(
        norm_inf_b > 0.0 && norm_inf_b.is_finite(),
        "eigensolve: ‖B‖_∞ = {norm_inf_b} — the pencil's λ-space resolution \
         floor is undefined for a degenerate B, and a degenerate B is a caller \
         bug, not a property of the shift",
    );

    shifted.nrows() as f64 * f64::EPSILON * norm_inf_shifted / norm_inf_b
}

/// Whether a σ≠0 solve's own output says `K − σB` was numerically singular.
///
/// Two ways it can say so, and BOTH are needed:
///
/// 1. **λ-space collapse.** A recovered λ within `lambda_floor` of σ is
///    indistinguishable from σ at this factorization's resolution, which is the
///    statement that `K − σB` is numerically singular.  This is the case
///    partial-pivot LU lets through as `Ok`: the pivot is tiny but non-zero, so
///    nothing upstream complains and the solve returns a plausible spectrum with
///    one entry pinned to σ and the rest quietly wrong.
/// 2. **An empty or non-finite spectrum.** At σ≠0 an empty result is a FAILURE,
///    not a silent answer: every μ was filtered out, so no λ was recovered at
///    all and there is nothing to report but the shift.
///
/// # Arm 2 CONFLATES two causes, knowingly
///
/// "σ sits on an eigenvalue" and "this σ≠0 solve converged nothing" are
/// different facts, and arm 2 reports both as the same
/// [`ShiftInvertFailure::ShiftAtEigenvalue`] carrying the same "move σ" remedy.
/// That is a deliberate v1 limit, not an oversight: an empty spectrum at σ≠0
/// leaves nothing to discriminate on, and inventing a second failure arm without
/// a way to tell them apart would only move the guess.  The behaviour is PINNED
/// by `empty_and_non_finite_spectra_are_conflated_with_a_singular_shift`, so
/// #7617 — which owns the split — inherits a measured baseline, not an
/// assumption.
///
/// How far arm 2 actually REACHES was measured, not assumed, and the answer is
/// "not observed end to end". Starving the 80-DOF fixture-C solve at a healthy
/// σ still converges 2 of 2 modes at `max_restarts = 1` and `tol = 1e-300`, so
/// no cheap `(K, B, σ)` that reaches this arm through
/// [`try_solve_eigen_shift_invert`] is known. The arm is therefore pinned at the
/// predicate rather than through a contrived pencil — the honest place, given
/// that the behaviour being recorded is a limit rather than a feature.
///
/// **No automatic perturbation, ever.** Nudging σ and re-solving is exactly the
/// silent-substitution class this contract exists to close: it would answer a
/// question the caller did not ask and label the result as though it had.  If it
/// is ever wanted it arrives as an explicit opt-in knob, never as a default.
fn shift_is_numerically_singular(eigenvalues: &[f64], sigma: f64, lambda_floor: f64) -> bool {
    if eigenvalues.is_empty() || eigenvalues.iter().any(|lam| !lam.is_finite()) {
        return true;
    }
    eigenvalues
        .iter()
        .any(|&lam| (lam - sigma).abs() <= lambda_floor)
}

/// Whether the Krylov window would reach the problem dimension, so the caller
/// must route to the dense solver instead.
///
/// faer's `partial_self_adjoint_eigen_imp` requires `max_dim < n` strictly, and
/// the public wrapper silently clamps `max_dim = min(max(params.max_dim,
/// max(2·MIN_DIM, 2·n_eigval)), n)` with `MIN_DIM = 32` (a faer constant).  For
/// n ≤ 64 (or 2·n_modes ≥ n) `max_dim` reaches n, causing a panic in the inner
/// thick-restart loop.  This mirrors faer's computation.
///
/// `FAER_MIN_DIM` mirrors the private `MIN_DIM` constant from faer-0.24
/// (`src/operator/eigen/mod.rs`).  If the faer workspace dependency is bumped,
/// re-check this value.  The `shift_invert_no_panic_at_min_dim_boundaries`
/// integration test sweeps every n in 2..=128 to catch silent divergence from
/// faer's actual floor without requiring a recompile.
fn routes_to_dense_fallback(n: usize, n_modes: usize) -> bool {
    const FAER_MIN_DIM: usize = 32; // faer-0.24
    let max_dim = (2 * n_modes).max(32).min(n);
    let effective_max_dim = max_dim.max(2 * FAER_MIN_DIM).max(2 * n_modes).min(n);
    effective_max_dim >= n
}

/// Replace the generic core's conservative C5 answer with the one this module's
/// dispatch can ESTABLISH from which factorization succeeded.
///
/// Rebuilt immutably rather than mutated after the fact: the core's result is a
/// value, and a caller reading the intermediate would see a field this module
/// already knows to be wrong.
///
/// [`lanczos_shift_invert`] keeps [`conservative_shift_provenance`] and must:
/// it is handed an opaque pre-built factorization and has no evidence to
/// establish `false`, so C5 forbids it assuming one.  Only here, where the
/// factorization was BUILT, does the evidence exist.
fn with_provenance(
    result: EigenSolverResult,
    sigma: f64,
    cholesky_succeeded: bool,
) -> EigenSolverResult {
    EigenSolverResult {
        shift_skipped_modes: shift_provenance_from_factorization(
            sigma,
            cholesky_succeeded,
        ),
        ..result
    }
}

/// Fixtures shared by this module's own tests, the crate's integration tests,
/// and `reify-eval`'s in-crate modal tests.
///
/// `#[doc(hidden)] pub` rather than `#[cfg(test)]` for the same reason
/// [`crate::assembly::test_support`] is: an integration test compiles against
/// the built library, so a `#[cfg(test)]` item is invisible to it and every
/// consumer ends up with its own copy. The closed form in particular was
/// maintained in four places before this seam existed.
#[doc(hidden)]
pub mod test_support {
    use faer::sparse::{SparseRowMat, Triplet};

    /// The `n`-DOF 1-D Dirichlet Laplacian pencil: `K = tridiag(−1, 2, −1)`
    /// (symmetric positive definite) with `B = I`.
    ///
    /// Its spectrum is closed-form ([`laplacian_lambda`]), which is what lets a
    /// test place σ EXACTLY on an eigenvalue in f64 rather than near one to
    /// within whatever tolerance some prior solve happened to reach.
    ///
    /// `n > 64` is the threshold above which [`super::routes_to_dense_fallback`]
    /// is false and the real Lanczos path runs.
    pub fn laplacian_pencil(n: usize) -> (SparseRowMat<usize, f64>, SparseRowMat<usize, f64>) {
        let mut k_trips = Vec::with_capacity(3 * n - 2);
        for i in 0..n {
            k_trips.push(Triplet::new(i, i, 2.0));
            if i > 0 {
                k_trips.push(Triplet::new(i, i - 1, -1.0));
            }
            if i + 1 < n {
                k_trips.push(Triplet::new(i, i + 1, -1.0));
            }
        }
        (
            SparseRowMat::try_new_from_triplets(n, n, &k_trips).unwrap(),
            identity(n),
        )
    }

    /// `I` (`n`×`n`), as a sparse row matrix.
    pub fn identity(n: usize) -> SparseRowMat<usize, f64> {
        let trips: Vec<Triplet<usize, usize, f64>> =
            (0..n).map(|i| Triplet::new(i, i, 1.0)).collect();
        SparseRowMat::try_new_from_triplets(n, n, &trips).unwrap()
    }

    /// Closed-form `λ_k = 2(1 − cos(kπ/(n+1)))` of [`laplacian_pencil`],
    /// 1-INDEXED (λ₁ is the first mode, not λ₀), matching the formula.
    pub fn laplacian_lambda(n: usize, k: usize) -> f64 {
        assert!(
            (1..=n).contains(&k),
            "an n = {n} pencil has modes k = 1..={n}, not {k}",
        );
        2.0 * (1.0 - f64::cos(k as f64 * std::f64::consts::PI / (n as f64 + 1.0)))
    }

    /// The `m` smallest closed-form eigenvalues of [`laplacian_pencil`],
    /// ascending.
    pub fn laplacian_lambdas<const M: usize>(n: usize) -> [f64; M] {
        std::array::from_fn(|i| laplacian_lambda(n, i + 1))
    }
}

// ---------------------------------------------------------------------------
// Unit tests for the private K − σB assembly
//
// In-crate because `shifted_pencil` is private: this is the same shape
// `sparse_util.rs` already uses for `find_in_row`.  Nothing reaches in from
// outside the module — the PUBLIC contract these support is pinned from
// `tests/eigensolve_shift_contract.rs` instead.
// ---------------------------------------------------------------------------

/// Unit tests for the private C6 predicate.
///
/// In-crate for the same reason `shifted_pencil_tests` below is: the item is
/// private, and its PUBLIC consequences are pinned from
/// `tests/eigensolve_shift_contract.rs`. What these add is the one thing the
/// public surface cannot show — which of the predicate's two arms answered —
/// so the known conflation is recorded as a measured baseline for δ (#7261)
/// instead of as a comment.
#[cfg(test)]
mod singular_shift_predicate_tests {
    use super::shift_is_numerically_singular;

    /// Both halves of arm 1, and the fact that they are INDISTINGUISHABLE from
    /// arm 2 at this interface.
    ///
    /// An empty spectrum and a non-finite one are "this solve produced nothing
    /// usable", not "σ sits on an eigenvalue" — different facts, one answer, one
    /// "move σ" remedy downstream. Pinned so that splitting them (δ's) shows up
    /// as a deliberate movement of this test rather than as a quietly different
    /// diagnostic reaching an author.
    #[test]
    fn empty_and_non_finite_spectra_are_conflated_with_a_singular_shift() {
        let floor = 1e-14;
        assert!(
            shift_is_numerically_singular(&[], 0.5, floor),
            "arm 1: an empty spectrum at σ≠0 is reported as a singular shift",
        );
        assert!(
            shift_is_numerically_singular(&[f64::NAN], 0.5, floor),
            "arm 1: a non-finite λ is reported as a singular shift",
        );
        assert!(
            shift_is_numerically_singular(&[f64::INFINITY, 0.1], 0.5, floor),
            "arm 1 fires on ANY non-finite entry, not only on an all-bad set",
        );
        assert!(
            shift_is_numerically_singular(&[0.5 + 0.5 * floor, 2.0], 0.5, floor),
            "arm 2: a λ inside the floor of σ is the genuine λ-space collapse",
        );
    }

    /// The negative half: a spectrum that stands clear of σ is NOT refused.
    ///
    /// Without this, a predicate that answered `true` unconditionally would
    /// satisfy every assertion above — and would refuse every shifted solve.
    #[test]
    fn a_spectrum_clear_of_sigma_is_not_a_singular_shift() {
        let floor = 1e-14;
        assert!(!shift_is_numerically_singular(&[0.1, 2.0], 0.5, floor));
        assert!(
            !shift_is_numerically_singular(&[0.5 + 2.0 * floor], 0.5, floor),
            "the floor is a threshold, not a neighbourhood the guard rounds up",
        );
    }
}

#[cfg(test)]
mod shifted_pencil_tests {
    use super::*;
    use faer::sparse::Triplet;

    /// A 4×4 pencil whose two sparsity patterns differ in BOTH directions.
    ///
    /// - `(0,1)` and `(1,0)` are in K only,
    /// - `(2,3)` and `(3,2)` are in B only,
    /// - the four diagonal entries are in both.
    ///
    /// All three cases have to be present at once: a merge that silently drops
    /// the operand-only entries still gets the shared ones right, and one that
    /// keeps only the left operand's pattern still gets K's own entries right.
    fn pencil_with_patterns_differing_in_both_directions()
    -> (SparseRowMat<usize, f64>, SparseRowMat<usize, f64>) {
        let k = SparseRowMat::try_new_from_triplets(
            4,
            4,
            &[
                Triplet::new(0, 0, 4.0),
                Triplet::new(0, 1, 1.0),
                Triplet::new(1, 0, 1.0),
                Triplet::new(1, 1, 5.0),
                Triplet::new(2, 2, 6.0),
                Triplet::new(3, 3, 7.0),
            ],
        )
        .unwrap();
        let b = SparseRowMat::try_new_from_triplets(
            4,
            4,
            &[
                Triplet::new(0, 0, 1.0),
                Triplet::new(1, 1, 1.0),
                Triplet::new(2, 2, 1.0),
                Triplet::new(2, 3, 2.0),
                Triplet::new(3, 2, 2.0),
                Triplet::new(3, 3, 1.0),
            ],
        )
        .unwrap();
        (k, b)
    }

    /// σ chosen to be neither 0 nor 1, so neither a dropped coefficient
    /// (`K − B`) nor an ignored one (`K`) can pass any assertion below.
    const SIGMA: f64 = 0.25;

    /// Collect the result's STORED entries as `(row, col, value)`, in column
    /// order.  Stored-ness is the point: an entry the arithmetic cancels is
    /// still stored (as an explicit zero) if the union covers it.
    fn stored_entries(m: &SparseColMat<usize, f64>) -> Vec<(usize, usize, f64)> {
        let m_ref = m.as_ref();
        let sym = m_ref.symbolic();
        let mut out = Vec::new();
        for j in 0..m.ncols() {
            let rows = sym.row_idx_of_col_raw(j);
            let vals = m_ref.val_of_col(j);
            for (&i, &v) in rows.iter().zip(vals.iter()) {
                out.push((i, j, v));
            }
        }
        out
    }

    /// (a) Every stored entry is exactly `k[i][j] − σ·b[i][j]`, with an absent
    /// operand entry read as zero.
    #[test]
    fn every_entry_is_k_minus_sigma_b() {
        let (k, b) = pencil_with_patterns_differing_in_both_directions();
        let shifted = shifted_pencil(&k, &b, SIGMA);

        let k_dense = k.to_dense();
        let b_dense = b.to_dense();
        for (i, j, got) in stored_entries(&shifted) {
            let want = k_dense[(i, j)] - SIGMA * b_dense[(i, j)];
            assert_eq!(
                got, want,
                "shifted_pencil[{i}][{j}] = {got}, expected k − σ·b = {} − {SIGMA}·{} = {want}",
                k_dense[(i, j)],
                b_dense[(i, j)],
            );
        }
    }

    /// (b) The sparsity pattern is exactly the UNION of K's and B's — an entry
    /// present only in B is stored (as `−σ·b`), and one present only in K is
    /// stored unchanged.
    #[test]
    fn pattern_is_exactly_the_union_of_both_operands() {
        let (k, b) = pencil_with_patterns_differing_in_both_directions();
        let shifted = shifted_pencil(&k, &b, SIGMA);

        let mut got: Vec<(usize, usize)> = stored_entries(&shifted)
            .into_iter()
            .map(|(i, j, _)| (i, j))
            .collect();
        got.sort_unstable();

        // The union, written out rather than recomputed from the operands, so a
        // merge bug cannot be masked by the same bug in the expectation.
        let mut want = vec![
            (0, 0),
            (0, 1),
            (1, 0),
            (1, 1),
            (2, 2),
            (2, 3),
            (3, 2),
            (3, 3),
        ];
        want.sort_unstable();
        assert_eq!(
            got, want,
            "shifted_pencil must store the UNION of the two patterns",
        );

        // Spot the two directions explicitly, with values, so a pattern that is
        // right while the arithmetic is not still reds here.
        let entries = stored_entries(&shifted);
        let at = |i: usize, j: usize| {
            entries
                .iter()
                .find(|&&(r, c, _)| r == i && c == j)
                .unwrap_or_else(|| panic!("entry ({i},{j}) is missing from the union"))
                .2
        };
        assert_eq!(
            at(0, 1),
            1.0,
            "an entry present only in K must survive unchanged (b[0][1] is absent, so σ·b = 0)",
        );
        assert_eq!(
            at(2, 3),
            -SIGMA * 2.0,
            "an entry present only in B must be STORED as −σ·b, not dropped",
        );
        assert_eq!(
            at(1, 1),
            5.0 - SIGMA,
            "an entry present in both must combine as k − σ·b",
        );
    }

    /// (c) The result is square, with the operands' dimensions.
    #[test]
    fn result_is_square_with_the_operand_dimensions() {
        let (k, b) = pencil_with_patterns_differing_in_both_directions();
        let shifted = shifted_pencil(&k, &b, SIGMA);
        assert_eq!(shifted.nrows(), 4, "row count must match the operands'");
        assert_eq!(shifted.ncols(), 4, "column count must match the operands'");
    }
}
