//! Jacobi-preconditioned conjugate-gradient (CG) solver for the SPD system
//! `K u = f` produced by the global stiffness assembly, Dirichlet BCs, and
//! Neumann BCs. See PRD `docs/prds/v0_3/structural-analysis-fea.md` task #12.
//!
//! # Two execution modes
//!
//! - [`SolverMode::Deterministic`] — single-threaded; sequential pairwise-tree
//!   reductions in slice order. Bit-stable across runs **and across machines**.
//! - [`SolverMode::Parallel { threads }`][SolverMode::Parallel] — row-partitioned
//!   SpMV via `std::thread::scope`; per-thread sequential pairwise-tree reductions;
//!   cross-thread combine in fixed handle order. Bit-stable per fixed thread count;
//!   tolerance-equivalent across thread counts.

use std::sync::Arc;

use faer::sparse::SparseRowMat;

/// How [`solve_cg`] parallelises the SpMV and dot-product reductions.
///
/// Mirrors [`crate::assembly::AssemblyMode`] byte-for-byte so the caller-side
/// wiring at PRD task #16 can hold a single mode enum and pass it to both
/// assemble and solve.
///
/// # `Deterministic`
///
/// Single-threaded, sequential pairwise-tree reductions in slice order.
/// Bit-stable across runs **and across machines**.
///
/// # `Parallel { threads }`
///
/// Row-partitioned SpMV via `std::thread::scope`. The row range `0..n` is
/// partitioned into `threads` contiguous chunks via `n.div_ceil(threads).max(1)`.
/// Per-thread sequential pairwise-tree reductions; cross-thread combine in fixed
/// handle order (spawn order). Bit-stable per fixed thread count; tolerance-
/// equivalent across thread counts.
///
/// Three mechanisms guarantee bit-stability per fixed thread count:
///
/// (a) **Chunk size** `n.div_ceil(threads).max(1)` is a deterministic function
///     of `(n, threads)` only — no work-stealing or load-balancing. Switching
///     to a dynamic scheduler would break the contract.
/// (b) **Sequential spawn** in chunk-iteration order; handle slot `t` always
///     corresponds to the worker for chunk `t`, regardless of OS thread ID.
/// (c) **Join in t-ascending order**; the cross-thread combine runs
///     `pairwise_tree_sum` over the spawn-ordered partial-sums Vec so the
///     final tree shape is a deterministic function of `(n, threads)`.
///
/// **Small-problem short-circuit**: when `n < PAR_THRESHOLD` (1024 DOFs),
/// the parallel helpers delegate to their sequential counterparts to avoid
/// thread-spawn overhead that would dominate the arithmetic. This strengthens
/// the determinism contract: for small n, `Parallel { threads: t }` produces
/// bit-identical results to `Deterministic` for any `t`.
///
/// Corollary: `Parallel { threads: 1 }` is exactly equivalent to
/// `Deterministic` — a single worker processes all `n` rows with the same
/// pairwise-tree shape as the sequential path. The
/// `parallel_disjoint_block_k_bit_equal_to_deterministic` test exercises this
/// at `t = 1` as the degenerate case.
///
/// Tests:
/// - `parallel_disjoint_block_k_bit_equal_to_deterministic` — bit-equality vs
///   Deterministic on block-diagonal K for `t ∈ {1, 2, 4}`. The test uses
///   n=16 < PAR_THRESHOLD so both modes take the sequential path; see test
///   comment for the conditions under which bit-equality holds for n ≥ PAR_THRESHOLD.
/// - `parallel_shared_dof_k_tolerance_equivalent_and_back_to_back_bit_stable`
///   — tolerance-equivalence vs Deterministic on shared-DOF fan-mesh K (`t=4`),
///   and back-to-back bit-stability for fixed `t=4`.
///
/// `Parallel { threads: 0 }` panics rather than auto-falling back to
/// single-threaded — auto-fallback would silently mask caller bugs (e.g. a
/// misread config defaulting `threads` to 0). The "tiny problems run
/// single-threaded under 10K DOFs" policy lives at the `ElasticOptions`
/// resolution layer (PRD task #16), not in this primitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolverMode {
    /// Single-threaded, pairwise-tree reductions in slice order.
    Deterministic,
    /// Row-partitioned SpMV with fixed-handle-order cross-thread combine.
    /// `threads` must be `>= 1`; passing `0` panics.
    Parallel {
        /// Worker thread count.
        threads: usize,
    },
}

/// Tuning parameters for [`solve_cg`].
#[derive(Debug, Clone, PartialEq)]
pub struct CgSolverOptions {
    /// Relative residual tolerance: converge when `‖r‖ < tolerance · ‖f‖`.
    /// Must be a finite positive value. Default: `1e-8`.
    pub tolerance: f64,
    /// Maximum number of CG iterations before giving up.
    /// Must be `>= 1`. Default: `1000`.
    pub max_iter: usize,
}

impl Default for CgSolverOptions {
    fn default() -> Self {
        Self {
            tolerance: 1e-8,
            max_iter: 1000,
        }
    }
}

/// Result returned by [`solve_cg`].
///
/// `iterations` counts the number of CG iterations *executed*:
/// - On convergence: `iterations` is the iteration on which the residual met
///   the tolerance. For a k×k SPD with the Jacobi preconditioner, this is at
///   most k iterations in exact arithmetic.
/// - On cap-out (`converged == false`): `iterations == max_iter` (the budget
///   was fully consumed; the solution `u` is the best iterate found).
/// - On zero RHS (`‖f‖ == 0`): `iterations == 0` (trivial exact solution
///   returned immediately; `u == 0` is exact).
///
/// The `max_iter_exhaustion_returns_unconverged` test pins the cap-out path.
#[derive(Debug, Clone, PartialEq)]
pub struct CgResult {
    /// Solution displacement vector (private backing store).
    ///
    /// Access via the stable public accessors:
    /// - `u(&self) -> &[f64]` — representation-agnostic read path; prefer
    ///   this over any direct field access.
    /// - `into_shared_u(self) -> Arc<Vec<f64>>` — consuming zero-copy
    ///   donation; use when you need an `Arc<Vec<f64>>` handle (e.g. to
    ///   populate a struct field) without copying the 10⁴–10⁶-DOF buffer.
    ///
    /// The field is private so the internal representation (`Arc<Vec<f64>>`,
    /// `Arc<[f64]>`, or a plain `Vec<f64>`) can be changed in a future
    /// refactor without breaking any external consumer that goes through
    /// the accessors.
    u: Arc<Vec<f64>>,
    /// Number of CG iterations executed.
    pub iterations: usize,
    /// `true` if the residual met the tolerance criterion before `max_iter`.
    pub converged: bool,
}

impl CgResult {
    /// Read the solution vector as a slice.
    ///
    /// Returns a `&[f64]` view over the solution, coercing via `Arc<Vec<f64>>`'s
    /// `Deref` chain. This is the representation-agnostic read path: callers
    /// should prefer `u()` over direct field access so the internal storage type
    /// can later change without breaking external consumers.
    pub fn u(&self) -> &[f64] {
        &self.u
    }

    /// Donate the solution allocation to the caller without copying.
    ///
    /// Consumes `self` and returns the `Arc<Vec<f64>>` directly. The caller
    /// receives sole ownership of this handle; if no other `Arc` clones exist,
    /// the allocation is uniquely owned by the returned `Arc`.
    ///
    /// Use this when you need to pass the solution into a struct field typed
    /// `Arc<Vec<f64>>` (e.g. `CantileverFeaSolve.u`) without a 10⁴–10⁶-DOF copy.
    /// For the representation-agnostic read path, use [`u()`](Self::u) instead.
    pub fn into_shared_u(self) -> Arc<Vec<f64>> {
        self.u
    }

    /// Clone the internal `Arc` without consuming `self`.
    ///
    /// Bumps the reference count and returns a new handle to the same allocation.
    /// Required by [`solve_cg_with_warm_state`] which must return both the
    /// `CgResult` and a [`crate::CgWarmState`] sharing one allocation — the
    /// consuming [`into_shared_u`](Self::into_shared_u) cannot serve a dual-return.
    ///
    /// Kept `pub(crate)` so the public surface is exactly the two accessors the
    /// task specifies ([`u()`](Self::u) and [`into_shared_u`](Self::into_shared_u)).
    pub(crate) fn shared_u(&self) -> Arc<Vec<f64>> {
        Arc::clone(&self.u)
    }
}

/// Return value of the per-iteration progress callback passed to
/// [`solve_cg_with_progress`].
///
/// Cooperative-cancellation primitive for the CG kernel. The callback returns
/// `Continue` to allow the next iteration, or `Cancel` to stop the solve
/// immediately. When `Cancel` is returned, `cg_loop` exits with
/// `iterations = iter_just_completed, converged = false`. The callback is
/// **not** invoked again after it returns `Cancel`.
///
/// # Why an enum rather than `bool`?
///
/// A `bool` return forces readers to memorise "true means cancel" vs "true means
/// continue" — a known footgun (cf. `Iterator::any` vs `Iterator::for_each`
/// predicates). `CgIterationControl` is self-documenting at the call site:
/// `if h.is_cancelled() { Cancel } else { Continue }` reads unambiguously.
///
/// Design decision recorded in `docs/prds/v0_3/gui-event-channel-inventory.md`
/// §11 Q2 / compute-node-contract §2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CgIterationControl {
    /// Allow the next CG iteration to proceed.
    Continue,
    /// Stop the CG solve immediately; the iteration that returned `Cancel` is
    /// counted in `CgResult.iterations` and `converged` is set to `false`.
    Cancel,
}

/// Solve the SPD linear system `K u = f` with Jacobi-preconditioned CG.
///
/// # Algorithm
///
/// Jacobi (diagonal) preconditioner: `M = diag(K)`. The preconditioned CG
/// iteration is:
///
/// ```text
/// r₀ = f − K u₀   (u₀ = 0 ⟹ r₀ = f)
/// z₀ = M⁻¹ r₀
/// p₀ = z₀
/// for k = 0, 1, …, max_iter − 1:
///     Kp  = K · p_k
///     α   = (r_k · z_k) / (p_k · Kp)
///     u_{k+1} = u_k + α p_k
///     r_{k+1} = r_k − α Kp
///     if ‖r_{k+1}‖² < tol² · ‖f‖²  →  converged
///     z_{k+1} = M⁻¹ r_{k+1}
///     β   = (r_{k+1} · z_{k+1}) / (r_k · z_k)
///     p_{k+1} = z_{k+1} + β p_k
/// ```
///
/// Special case: if `‖f‖² == 0.0`, return `u = 0` immediately with
/// `iterations = 0, converged = true` (trivial exact solution; avoids `0/0`
/// in the relative tolerance check).
///
/// # Determinism contract (Deterministic mode)
///
/// Identical inputs produce bit-for-bit identical outputs **on the same
/// machine and across machines**. The `deterministic_back_to_back_bit_stable`
/// test pins this contract as a regression guard. Three mechanisms guarantee
/// it:
///
/// 1. **Single-threaded execution** — no thread-scheduling order dependence.
/// 2. **Pairwise-tree reductions** — the tree shape is a deterministic
///    function of input length only; `pairwise_tree_sum_fn` recurses by halving
///    with a fixed base case of ≤ 8 elements, so the same `len` always
///    produces the same reduction order regardless of scheduling.
/// 3. **Slot-order vector ops** — `u += α p`, `r -= α Kp`, `p = z + β p`
///    iterate slot `0 → n−1` in slice order; no iteration-order dependence.
///
/// # Panics
///
/// - `SolverMode::Parallel { threads: 0 }` — auto-fallback would silently
///   mask caller bugs; the panic surfaces them at the call site.
/// - `f.len() != k.nrows()` — the RHS vector must be sized to the system.
/// - `k.nrows() != k.ncols()` — `K` must be square.
/// - Any row `i` of `K` has no stored diagonal entry or has `K[i][i] == 0.0`
///   (Jacobi preconditioner is undefined without a non-zero diagonal).
/// - `opts.tolerance` is not finite or not positive — non-positive tolerance
///   makes the convergence check `‖r‖² < tol² · ‖f‖²` unreachable; infinite
///   tolerance makes it trivially satisfied on the first iteration; NaN makes
///   the comparison undefined.
/// - `opts.max_iter == 0` — the iteration loop runs zero times and would
///   return `iterations == 0, converged == false`, colliding with the
///   `CgResult` zero-RHS guarantee that `iterations == 0 ⟹ converged: true`.
///
/// Per the Task-2544 contract-explicitness convention: all panics use
/// unconditional `assert!` (not `debug_assert!`) with descriptive messages
/// naming the offending values.
pub fn solve_cg(
    k: &SparseRowMat<usize, f64>,
    f: &[f64],
    opts: CgSolverOptions,
    mode: SolverMode,
) -> CgResult {
    // Backward-compatible cold-start entry: delegates to `solve_cg_warm` with
    // `initial_guess = None`, which initializes `u₀ = 0` and `r₀ = f` exactly
    // as the original code did. Every existing caller and test (including the
    // determinism-contract regression tests) continues to produce bit-for-bit
    // identical output. Pinned by `solve_cg_warm_with_none_matches_solve_cg_bit_for_bit`.
    solve_cg_warm(k, f, None, opts, mode)
}

/// Solve the SPD linear system `K u = f` with Jacobi-preconditioned CG, with
/// an optional initial guess for warm-starting the iteration (PRD task #14).
///
/// When `initial_guess` is `None`, behaves identically to [`solve_cg`]:
/// `u₀ = 0`, `r₀ = f`, no SpMV is performed for residual seeding, and the
/// deterministic-mode bit-equality contract is preserved.
///
/// When `initial_guess` is `Some(u₀)`, the CG iteration starts from
/// `u = u₀` and `r = f − K·u₀` (one extra SpMV at the start). If `‖r₀‖²`
/// already meets the convergence threshold, returns immediately with
/// `iterations = 0, converged = true` — symmetric with the existing zero-RHS
/// short-circuit and avoiding the `0/0` from `α = rz / pkp` when `rz ≈ 0`.
///
/// # Iteration-reduction contract
///
/// For warm starts where `u₀ ≈ u_exact`, CG converges in fewer iterations
/// than the cold start. CG with a near-correct initial guess naturally
/// converges faster because the seeded residual `r₀ = f − K·u₀` is smaller
/// than the cold `r₀ = f`, so the convergence threshold is reached sooner.
/// No additional code is needed beyond the warm-start initialization.
/// Pinned by `warm_start_with_perturbed_rhs_reduces_iteration_count` as
/// the regression guard.
///
/// # Panics
///
/// All `solve_cg` panic conditions plus:
/// - `initial_guess.is_some() && initial_guess.unwrap().len() != k.nrows()`
///   — the initial guess must be sized to the system.
pub fn solve_cg_warm(
    k: &SparseRowMat<usize, f64>,
    f: &[f64],
    initial_guess: Option<&[f64]>,
    opts: CgSolverOptions,
    mode: SolverMode,
) -> CgResult {
    solve_cg_impl(k, f, initial_guess, opts, mode, None)
}

/// Solve the SPD linear system `K u = f` with Jacobi-preconditioned CG,
/// invoking a per-iteration progress callback after each residual-norm update.
///
/// Identical contract to [`solve_cg_warm`] except for the additional `progress`
/// closure. The closure is invoked at the **end** of each CG iteration (after the
/// residual-norm update and before the convergence branch) with
/// `(iter_just_completed: usize, residual_l2_norm: f64)`. `iter_just_completed`
/// is 1-indexed: the first iteration fires `(1, ‖r₁‖)`.
///
/// The return value of the callback is [`CgIterationControl`]:
/// - [`CgIterationControl::Continue`] — allow the next iteration.
/// - [`CgIterationControl::Cancel`] — exit immediately with
///   `iterations = iter_just_completed, converged = false`.
///   **Note:** Cancel is checked *before* the convergence branch — if the
///   callback returns `Cancel` on an iteration whose residual would have
///   satisfied the convergence tolerance, the result is `converged = false`
///   anyway. Callers that want "cancel only if not yet converged" should check
///   `residual_l2_norm` against their own tolerance before returning `Cancel`.
///
/// The callback is **not** invoked after it returns `Cancel`, and is also not
/// invoked when `cg_loop` exits early via the warm-start `‖r₀‖² < tol_sq`
/// check (returns `iterations = 0, converged = true` without entering the main
/// loop).
///
/// # Use in engine-boundary wiring (GR-016 ζ)
///
/// The GUI side translates `CancellationHandle::is_cancelled()` into
/// `CgIterationControl::Cancel` at the engine boundary. The iteration callback
/// also emits the `solver-progress` Tauri event via `event_bus::emit_typed`.
/// Both are out of scope for this crate — this function is the kernel-side seam.
///
/// See `docs/gui-event-channels/solver-progress.md` §3 for the producer-side spec.
///
/// # Panics
///
/// Same panic conditions as [`solve_cg_warm`].
pub fn solve_cg_with_progress(
    k: &SparseRowMat<usize, f64>,
    f: &[f64],
    initial_guess: Option<&[f64]>,
    opts: CgSolverOptions,
    mode: SolverMode,
    progress: &mut dyn FnMut(usize, f64) -> CgIterationControl,
) -> CgResult {
    solve_cg_impl(k, f, initial_guess, opts, mode, Some(progress))
}

/// Shared implementation backing [`solve_cg_warm`] and [`solve_cg_with_progress`].
///
/// Contains all contract checks, the zero-RHS short-circuit, Jacobi-preconditioner
/// setup, and mode-dispatch to [`cg_loop`]. The `progress` parameter is forwarded
/// directly to `cg_loop`; callers pass `None` (no-op, no overhead) or `Some(cb)`
/// (per-iteration callback). Public function signatures remain unchanged.
fn solve_cg_impl(
    k: &SparseRowMat<usize, f64>,
    f: &[f64],
    initial_guess: Option<&[f64]>,
    opts: CgSolverOptions,
    mode: SolverMode,
    progress: Option<&mut dyn FnMut(usize, f64) -> CgIterationControl>,
) -> CgResult {
    // --- Contract checks (per Task-2544 contract-explicitness convention) ---
    //
    // Zero-threads check first (before dim checks): a Parallel { threads: 0 }
    // call with any input shape should panic, surfacing the caller bug
    // regardless of problem size.
    if let SolverMode::Parallel { threads } = mode {
        assert!(
            threads != 0,
            "SolverMode::Parallel {{ threads: 0 }} is invalid: \
             auto-fallback to single-threaded would silently mask caller bugs \
             (e.g. a misread config defaulting threads to 0). \
             Pass threads >= 1, or use SolverMode::Deterministic for \
             single-threaded pairwise-tree reductions.",
        );
    }
    assert!(
        opts.tolerance.is_finite() && opts.tolerance > 0.0,
        "CgSolverOptions.tolerance = {} must be a finite positive value",
        opts.tolerance,
    );
    assert!(
        opts.max_iter > 0,
        "CgSolverOptions.max_iter = 0 is invalid; must be >= 1",
    );
    assert_eq!(
        f.len(),
        k.nrows(),
        "f.len() = {} but k.nrows() = {}; f must be sized to the system (f.len() == k.nrows())",
        f.len(),
        k.nrows(),
    );
    assert_eq!(
        k.nrows(),
        k.ncols(),
        "K must be square: k.nrows() = {} but k.ncols() = {}; \
         the stiffness matrix must be n × n",
        k.nrows(),
        k.ncols(),
    );
    // Pinned by `initial_guess_length_mismatch_panics`.
    if let Some(u_0) = initial_guess {
        assert_eq!(
            u_0.len(),
            k.nrows(),
            "initial_guess.len() = {} but k.nrows() = {}; \
             initial_guess must be sized to the system (initial_guess.len() == k.nrows())",
            u_0.len(),
            k.nrows(),
        );
    }

    let n = f.len();

    // --- Jacobi preconditioner: extract diagonal of K ---
    let inv_diag = extract_diag_jacobi(k);

    // --- Special case: zero RHS ---
    // ‖f‖² == 0.0 ⟹ u = 0 is the exact solution to K·u = 0 (K SPD ⟹ trivial
    // nullspace). Return u = 0 immediately regardless of any initial guess —
    // honouring the initial guess and iterating would still converge to u = 0,
    // but the relative-tolerance check uses tol² · ‖f‖² = 0 which would never
    // trip and the solver would cap out at max_iter. (Unconditional == 0.0 is
    // safe here: f is the caller's vector and pairwise_tree_sum of zeros
    // is deterministically 0.0.)
    //
    // f_norm_sq is computed once before dispatch; sequential norm2_squared
    // is appropriate here because: (1) this is a one-shot computation
    // outside the hot loop, so parallel spawn overhead would be wasted;
    // (2) norm2_squared is bit-identical to norm2_squared_parallel(threads=1)
    // on the same input, so the convergence threshold is mode-invariant.
    let f_norm_sq = norm2_squared(f);
    if f_norm_sq == 0.0 {
        return CgResult {
            u: Arc::new(vec![0.0; n]),
            iterations: 0,
            converged: true,
        };
    }
    let tol_sq = opts.tolerance * opts.tolerance * f_norm_sq;

    // --- Build initial (u, r) pair ---
    //
    // None branch: u = 0, r = f (no SpMV, no FP reordering). Bit-identical to
    // the pre-warm-start code path — preserves the deterministic-mode
    // bit-equality contract.
    //
    // Some branch: u = u₀.to_vec(), r = f − K·u₀ via one extra mode-appropriate
    // SpMV. The mode-appropriate primitive is selected at the dispatch site
    // below to keep determinism-comment ownership with the primitive functions.
    //
    // --- Dispatch to unified CG loop with mode-appropriate primitives ---
    //
    // The Deterministic and Parallel paths share a single CG loop implementation
    // (`cg_loop`); the difference is purely which SpMV/dot/norm² closures are
    // passed in. Determinism-comment ownership stays with the primitive functions.
    match mode {
        SolverMode::Deterministic => {
            let (u, r) = build_initial_u_r(f, initial_guess, |p, out| spmv_seq(k, p, out));
            cg_loop(
                u,
                r,
                &inv_diag,
                tol_sq,
                opts.max_iter,
                |p, out| spmv_seq(k, p, out),
                dot,
                norm2_squared,
                progress,
            )
        }
        SolverMode::Parallel { threads } => {
            let (u, r) =
                build_initial_u_r(f, initial_guess, |p, out| spmv_parallel(k, p, out, threads));
            cg_loop(
                u,
                r,
                &inv_diag,
                tol_sq,
                opts.max_iter,
                |p, out| spmv_parallel(k, p, out, threads),
                |a, b| dot_parallel(a, b, threads),
                |v| norm2_squared_parallel(v, threads),
                progress,
            )
        }
    }
}

/// Build the initial `(u, r)` pair for the CG iteration based on the
/// optional initial guess.
///
/// - `None` branch: `u = vec![0.0; n]`, `r = f.to_vec()` — bit-identical to
///   the pre-warm-start code path (no SpMV, no FP reordering).
/// - `Some(u₀)` branch: `u = u₀.to_vec()`, `r = f − K·u₀` via one
///   mode-appropriate SpMV. Residual is computed via slot-order in-place
///   subtraction (`r[i] -= ku[i]`) matching `cg_loop`'s r-update
///   convention; residual is bit-exact to `f[i] - (K·u₀)[i]` per slot.
fn build_initial_u_r<S>(f: &[f64], initial_guess: Option<&[f64]>, spmv: S) -> (Vec<f64>, Vec<f64>)
where
    S: FnOnce(&[f64], &mut [f64]),
{
    let n = f.len();
    match initial_guess {
        None => (vec![0.0_f64; n], f.to_vec()),
        Some(u_0) => {
            let u = u_0.to_vec();
            // r = f − K·u₀ via slot-order in-place subtraction (matches cg_loop's
            // r-update convention; preserves bit-equality with the prior collect form
            // since each slot computes the same f[i] - ku[i] f64 op).
            let mut ku = vec![0.0_f64; n];
            spmv(&u, &mut ku);
            let mut r = f.to_vec();
            for i in 0..n {
                r[i] -= ku[i];
            }
            (u, r)
        }
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Minimum problem size (DOFs) below which the parallel helpers fall through
/// to their sequential counterparts, avoiding thread-spawn overhead that would
/// dominate the arithmetic. At n < PAR_THRESHOLD the sequential path already
/// completes in microseconds; spawning OS threads is measurably more expensive.
///
/// This threshold also strengthens the determinism contract: any
/// `Parallel { threads }` call with `n < PAR_THRESHOLD` produces bit-identical
/// results to `Deterministic` on the same input, because both take the
/// sequential code path.
const PAR_THRESHOLD: usize = 1024;

/// Pairwise-tree summation over `len` terms via a generator closure,
/// **without allocating an intermediate buffer**.
///
/// The tree shape is a deterministic function of `len` only — the same `len`
/// always produces the same reduction order regardless of scheduling. This is
/// the load-bearing mechanism for bit-stability across both sequential and
/// parallel modes.
///
/// # Performance
///
/// The generator `get` is called through `&dyn Fn` exactly once per element
/// regardless of recursion depth — recursion only threads `(start, len)`
/// parameters, the same `&dyn Fn` reference is forwarded unchanged through the
/// tree. `get` receives absolute indices into the caller's data via
/// `start + offset`. For len ≤ 8, the result is a fully inlined expression
/// with no recursion. This is significantly cheaper than allocating a
/// `Vec<f64>` of `len` products for each call in the hot path.
fn pairwise_tree_sum_fn(start: usize, len: usize, get: &dyn Fn(usize) -> f64) -> f64 {
    match len {
        0 => 0.0,
        1 => get(start),
        2 => get(start) + get(start + 1),
        3 => get(start) + get(start + 1) + get(start + 2),
        4 => (get(start) + get(start + 1)) + (get(start + 2) + get(start + 3)),
        5 => (get(start) + get(start + 1)) + (get(start + 2) + get(start + 3)) + get(start + 4),
        6 => {
            (get(start) + get(start + 1) + get(start + 2))
                + (get(start + 3) + get(start + 4) + get(start + 5))
        }
        7 => {
            (get(start) + get(start + 1) + get(start + 2) + get(start + 3))
                + (get(start + 4) + get(start + 5) + get(start + 6))
        }
        8 => {
            (get(start) + get(start + 1) + get(start + 2) + get(start + 3))
                + (get(start + 4) + get(start + 5) + get(start + 6) + get(start + 7))
        }
        _ => {
            let mid = len / 2;
            pairwise_tree_sum_fn(start, mid, get)
                + pairwise_tree_sum_fn(start + mid, len - mid, get)
        }
    }
}

/// Pairwise-tree summation over a slice.
///
/// Convenience wrapper around [`pairwise_tree_sum_fn`] for combining the
/// small `partials` Vec produced by parallel workers (at most `threads` entries).
fn pairwise_tree_sum(slice: &[f64]) -> f64 {
    pairwise_tree_sum_fn(0, slice.len(), &|i| slice[i])
}

/// Dot product `a · b` using pairwise-tree summation, without allocation.
///
/// Asserts `a.len() == b.len()`.
fn dot(a: &[f64], b: &[f64]) -> f64 {
    assert_eq!(
        a.len(),
        b.len(),
        "dot: len mismatch {} vs {}",
        a.len(),
        b.len()
    );
    pairwise_tree_sum_fn(0, a.len(), &|i| a[i] * b[i])
}

/// Squared Euclidean norm `‖v‖²` using pairwise-tree summation, without allocation.
fn norm2_squared(v: &[f64]) -> f64 {
    pairwise_tree_sum_fn(0, v.len(), &|i| v[i] * v[i])
}

/// Extract diagonal entries of `K` as a vector of inverse values `1/K[i][i]`.
///
/// Panics with a descriptive message naming the row index if any diagonal
/// entry is absent or is `0.0`. Per the Task-2544 contract-explicitness
/// convention: unconditional `assert!` so the contract is explicit in
/// production code.
fn extract_diag_jacobi(k: &SparseRowMat<usize, f64>) -> Vec<f64> {
    let (sym, vals) = k.parts();
    let row_ptr = sym.row_ptr();
    let col_idx = sym.col_idx();
    let n = sym.nrows();

    let mut inv_diag = Vec::with_capacity(n);
    for i in 0..n {
        let start = row_ptr[i];
        let end = row_ptr[i + 1];
        let mut found = false;
        for idx in start..end {
            if col_idx[idx] == i {
                let d = vals[idx];
                assert!(
                    d != 0.0,
                    "Jacobi preconditioner: row {i} has a stored diagonal entry K[{i}][{i}] = 0.0; \
                     the Jacobi preconditioner requires a non-zero diagonal at every row. \
                     Check that K is assembled correctly and has no unconstrained rigid-body modes.",
                );
                inv_diag.push(1.0 / d);
                found = true;
                break;
            }
        }
        assert!(
            found,
            "Jacobi preconditioner: row {i} has no stored diagonal entry K[{i}][{i}]; \
             the Jacobi preconditioner requires a non-zero diagonal at every row. \
             FEA-assembled K always has a diagonal entry per Task 2916; \
             a missing diagonal indicates the input K is not FEA-assembled.",
        );
    }
    inv_diag
}

/// Sequential CSR SpMV: `out[i] = Σ_j K[i,j] · p[j]`.
///
/// Uses [`pairwise_tree_sum_fn`] for each row's dot product to give
/// O(log nnz_per_row) error growth and deterministic reduction order,
/// **without allocating an intermediate buffer per row**.
fn spmv_seq(k: &SparseRowMat<usize, f64>, p: &[f64], out: &mut [f64]) {
    let (sym, vals) = k.parts();
    let row_ptr = sym.row_ptr();
    let col_idx = sym.col_idx();
    let n = sym.nrows();

    for i in 0..n {
        let start = row_ptr[i];
        let end = row_ptr[i + 1];
        // No Vec allocation — pairwise_tree_sum_fn calls the generator directly.
        out[i] = pairwise_tree_sum_fn(0, end - start, &|k| vals[start + k] * p[col_idx[start + k]]);
    }
}

/// Parallel row-partitioned SpMV: `out[i] = Σ_j K[i,j] · p[j]`.
///
/// Row range `0..n` is partitioned into `threads` contiguous chunks via
/// `n.div_ceil(threads).max(1)`. Each thread owns a disjoint mutable slice
/// of `out` (via `split_at_mut`). Per-row inner products use
/// [`pairwise_tree_sum_fn`] without allocation. Worker handles are joined in
/// spawn order.
///
/// **Small-problem short-circuit**: delegates to [`spmv_seq`] when
/// `n < PAR_THRESHOLD`, avoiding thread-spawn overhead that would dominate
/// the arithmetic at small n.
///
/// # Determinism contract
///
/// (a) Chunk size `n.div_ceil(threads).max(1)` is a deterministic function
///     of `(n, threads)` only — no work-stealing or load-balancing.
/// (b) Threads spawn sequentially in chunk-iteration order.
/// (c) Each thread's per-row pairwise-tree uses the same fixed-shape
///     recursion as `spmv_seq`, so the SpMV result is bit-stable per
///     fixed thread count.
fn spmv_parallel(k: &SparseRowMat<usize, f64>, p: &[f64], out: &mut [f64], threads: usize) {
    let (sym, vals) = k.parts();
    let n = sym.nrows();

    // Short-circuit: avoid thread-spawn overhead for small problems.
    // Also strengthens bit-stability: Parallel on n < PAR_THRESHOLD is
    // bit-identical to Deterministic on the same input.
    if n < PAR_THRESHOLD {
        return spmv_seq(k, p, out);
    }

    let row_ptr = sym.row_ptr();
    let col_idx = sym.col_idx();
    let chunk_size = n.div_ceil(threads).max(1);

    std::thread::scope(|s| {
        let mut handles = Vec::new();
        // Split `out` into disjoint mutable chunks in chunk order so that
        // each worker writes into its own slice without any locking.
        let mut remaining_out = &mut out[..];
        let mut row_start = 0;

        while row_start < n {
            let row_end = (row_start + chunk_size).min(n);
            let chunk_len = row_end - row_start;

            // `split_at_mut` gives us a disjoint borrow; `remaining_out`
            // now points to the tail for the next iteration.
            let (out_chunk, rest) = remaining_out.split_at_mut(chunk_len);
            remaining_out = rest;

            handles.push(s.spawn(move || {
                for (i, out_elem) in out_chunk.iter_mut().enumerate() {
                    let global_row = row_start + i;
                    let start_idx = row_ptr[global_row];
                    let end_idx = row_ptr[global_row + 1];
                    // No Vec allocation — pairwise_tree_sum_fn calls the generator directly.
                    *out_elem = pairwise_tree_sum_fn(0, end_idx - start_idx, &|k| {
                        vals[start_idx + k] * p[col_idx[start_idx + k]]
                    });
                }
            }));

            row_start = row_end;
        }

        for h in handles {
            match h.join() {
                Ok(()) => {}
                Err(payload) => std::panic::resume_unwind(payload),
            }
        }
    });
}

/// Parallel dot product `a · b` via chunk-partitioned pairwise-tree.
///
/// Each thread computes [`pairwise_tree_sum_fn`] on its chunk's element-wise
/// products (no allocation); partial sums are collected in spawn order and
/// combined via `pairwise_tree_sum`. Gives bit-stability per fixed thread count.
///
/// **Small-problem short-circuit**: delegates to sequential [`dot`] when
/// `n < PAR_THRESHOLD`.
fn dot_parallel(a: &[f64], b: &[f64], threads: usize) -> f64 {
    assert_eq!(
        a.len(),
        b.len(),
        "dot_parallel: len mismatch {} vs {}",
        a.len(),
        b.len()
    );
    let n = a.len();

    // Short-circuit for small problems — avoids spawn overhead.
    if n < PAR_THRESHOLD {
        return dot(a, b);
    }

    let chunk_size = n.div_ceil(threads).max(1);

    let partials: Vec<f64> = std::thread::scope(|s| {
        let mut handles = Vec::new();
        let mut start = 0;

        while start < n {
            let end = (start + chunk_size).min(n);
            let a_chunk = &a[start..end];
            let b_chunk = &b[start..end];

            // No Vec allocation — pairwise_tree_sum_fn calls the generator directly.
            handles.push(s.spawn(move || {
                pairwise_tree_sum_fn(0, a_chunk.len(), &|i| a_chunk[i] * b_chunk[i])
            }));

            start = end;
        }

        handles
            .into_iter()
            .map(|h| match h.join() {
                Ok(v) => v,
                Err(payload) => std::panic::resume_unwind(payload),
            })
            .collect()
    });

    pairwise_tree_sum(&partials)
}

/// Parallel squared Euclidean norm `‖v‖²` via chunk-partitioned pairwise-tree.
///
/// **Small-problem short-circuit**: delegates to sequential [`norm2_squared`]
/// when `n < PAR_THRESHOLD`.
fn norm2_squared_parallel(v: &[f64], threads: usize) -> f64 {
    let n = v.len();

    // Short-circuit for small problems — avoids spawn overhead.
    if n < PAR_THRESHOLD {
        return norm2_squared(v);
    }

    let chunk_size = n.div_ceil(threads).max(1);

    let partials: Vec<f64> = std::thread::scope(|s| {
        let mut handles = Vec::new();
        let mut start = 0;

        while start < n {
            let end = (start + chunk_size).min(n);
            let chunk = &v[start..end];

            // No Vec allocation — pairwise_tree_sum_fn calls the generator directly.
            handles.push(
                s.spawn(move || pairwise_tree_sum_fn(0, chunk.len(), &|i| chunk[i] * chunk[i])),
            );

            start = end;
        }

        handles
            .into_iter()
            .map(|h| match h.join() {
                Ok(v) => v,
                Err(payload) => std::panic::resume_unwind(payload),
            })
            .collect()
    });

    pairwise_tree_sum(&partials)
}

/// Core Jacobi-preconditioned CG iteration, parameterised by mode-specific
/// SpMV, dot, and norm² implementations.
///
/// The CG algorithm is identical for both Deterministic and Parallel modes;
/// only the reduction primitives differ. Accepting closures here eliminates
/// an ~80-line code clone between the two modes: a future bug fix or
/// convergence-criterion tweak is applied in one place. Determinism-comment
/// ownership stays with the primitive functions (`spmv_seq` / `spmv_parallel`,
/// `dot` / `dot_parallel`, `norm2_squared` / `norm2_squared_parallel`).
///
/// # Arguments
///
/// - `u` — initial iterate (consumed and returned in `CgResult.u`); cold
///   start passes `vec![0.0; n]`, warm start passes `u₀.to_vec()`.
/// - `r` — initial residual `f − K·u`; cold start passes `f.to_vec()`,
///   warm start passes `f − K·u₀` (one extra SpMV at the dispatch site).
/// - `inv_diag` — `1/K[i][i]` Jacobi preconditioner.
/// - `spmv(p, out)` — compute `out = K · p` for the current search direction.
/// - `dot_fn(a, b)` — dot product `a · b` with mode-appropriate reduction.
/// - `norm2sq_fn(v)` — squared norm `‖v‖²` with mode-appropriate reduction.
///
/// # Early-exit (warm-start `u₀ ≈ u_exact`)
///
/// Before the main iteration loop, checks if `‖r‖² < tol_sq`. If so, returns
/// `iterations = 0, converged = true` immediately — symmetric with the
/// `f_norm_sq == 0.0` short-circuit at the dispatch site, and avoids the
/// `0/0` from `α = rz / pkp` when `rz ≈ 0, pkp ≈ 0`. Pinned by
/// `warm_start_at_exact_solution_returns_in_zero_iterations`.
#[allow(clippy::too_many_arguments)] // 9 args: state (5) + mode-injected closures (3) + progress (1).
fn cg_loop<S, D, N>(
    mut u: Vec<f64>,
    mut r: Vec<f64>,
    inv_diag: &[f64],
    tol_sq: f64,
    max_iter: usize,
    spmv: S,
    dot_fn: D,
    norm2sq_fn: N,
    mut progress: Option<&mut dyn FnMut(usize, f64) -> CgIterationControl>,
) -> CgResult
where
    S: Fn(&[f64], &mut [f64]),
    D: Fn(&[f64], &[f64]) -> f64,
    N: Fn(&[f64]) -> f64,
{
    let n = u.len();
    // Early-exit (warm-start `u₀ ≈ u_exact`): if the seeded residual already
    // meets the convergence threshold, return without iterating. Symmetric
    // with the f_norm_sq == 0.0 short-circuit at the dispatch site; avoids
    // 0/0 in α = rz / pkp when rz ≈ 0. Pinned by
    // `warm_start_at_exact_solution_returns_in_zero_iterations`.
    //
    // Two residual notions: this check uses the *recomputed true residual*
    // `r₀ = f − K·u₀` from the dispatch site, while the in-loop convergence
    // check at the bottom of the iteration uses the *maintained residual*
    // (updated incrementally as `r ← r − α·Kp` each iteration). The two
    // drift apart over many iterations due to floating-point round-off.
    // Consequence: a warm-start where `u₀` came from a long cold solve on
    // a large/ill-conditioned system may have `‖f − K·u₀‖² > tol_sq` even
    // though the cold-solve maintained residual was below tol_sq at
    // convergence. That is acceptable — the loop simply does a few more
    // iterations to re-tighten the maintained residual. Future readers
    // should NOT "fix" this apparent inconsistency: it reflects the real
    // numerical state of the system at u₀, not a bug.
    if norm2sq_fn(&r) < tol_sq {
        return CgResult {
            u: Arc::new(u),
            iterations: 0,
            converged: true,
        };
    }

    // Allocate scratch vectors. All axpy ops iterate slot 0 → n−1 in slice order.
    // z₀ = M⁻¹ r₀
    let mut z: Vec<f64> = r
        .iter()
        .zip(inv_diag.iter())
        .map(|(ri, di)| ri * di)
        .collect();
    // p₀ = z₀
    let mut p: Vec<f64> = z.clone();
    // rz = r₀ · z₀
    let mut rz = dot_fn(&r, &z);

    let mut kp = vec![0.0_f64; n];

    for iter in 0..max_iter {
        // Kp = K · p_k
        spmv(&p, &mut kp);

        // α = (r_k · z_k) / (p_k · Kp)
        let pkp = dot_fn(&p, &kp);
        assert!(
            pkp > 0.0,
            "CG: p·Kp = {pkp} ≤ 0 at iteration {iter}; K must be positive-definite \
             and p must be a non-zero direction. This indicates a degenerate system.",
        );
        let alpha = rz / pkp;

        // u_{k+1} = u_k + α p_k
        for i in 0..n {
            u[i] += alpha * p[i];
        }
        // r_{k+1} = r_k − α Kp
        for i in 0..n {
            r[i] -= alpha * kp[i];
        }

        // Convergence check: ‖r_{k+1}‖² < tol² · ‖f‖²
        let r_norm_sq = norm2sq_fn(&r);

        // Per-iteration progress callback: fires post-residual-norm-update,
        // before the convergence branch, so the converging iteration is always
        // included in the callback sequence (observed.len() == result.iterations).
        // The L2 norm (not squared) is emitted — callers display/log residuals
        // in L2-norm units; doing the sqrt here avoids repeated sqrt at N consumers.
        //
        // Cooperative-cancellation contract (PRD §11 Q2 / compute-node-contract §2):
        // if the callback returns Cancel, exit immediately with
        // `iterations = iter + 1, converged = false`. The callback is NOT invoked
        // again after it returns Cancel; no further z/β/p updates are computed.
        if let Some(ref mut cb) = progress
            && cb(iter + 1, r_norm_sq.sqrt()) == CgIterationControl::Cancel
        {
            return CgResult {
                u: Arc::new(u),
                iterations: iter + 1,
                converged: false,
            };
        }

        if r_norm_sq < tol_sq {
            return CgResult {
                u: Arc::new(u),
                iterations: iter + 1,
                converged: true,
            };
        }

        // z_{k+1} = M⁻¹ r_{k+1}  (sequential — 1 mul per slot, no reduction)
        for i in 0..n {
            z[i] = r[i] * inv_diag[i];
        }

        // β = (r_{k+1} · z_{k+1}) / (r_k · z_k)
        let rz_new = dot_fn(&r, &z);
        let beta = rz_new / rz;
        rz = rz_new;

        // p_{k+1} = z_{k+1} + β p_k
        for i in 0..n {
            p[i] = z[i] + beta * p[i];
        }
    }

    // Cap-out without convergence.
    CgResult {
        u: Arc::new(u),
        iterations: max_iter,
        converged: false,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::needless_range_loop)] // index-parallel loops in test asserts
    use super::{
        CgIterationControl, CgSolverOptions, SolverMode, build_initial_u_r, norm2_squared,
        pairwise_tree_sum_fn, solve_cg, solve_cg_warm, solve_cg_with_progress, spmv_seq,
    };
    use faer::sparse::{SparseRowMat, Triplet};

    /// Build a tiny 1×1 identity sparse matrix for contract-panic tests.
    fn identity_1x1() -> SparseRowMat<usize, f64> {
        SparseRowMat::try_new_from_triplets(1, 1, &[Triplet::new(0_usize, 0_usize, 1.0_f64)])
            .unwrap()
    }

    /// Build a 2×2 identity sparse matrix.
    fn identity_2x2() -> SparseRowMat<usize, f64> {
        SparseRowMat::try_new_from_triplets(
            2,
            2,
            &[
                Triplet::new(0_usize, 0_usize, 1.0_f64),
                Triplet::new(1_usize, 1_usize, 1.0_f64),
            ],
        )
        .unwrap()
    }

    // --- Public-surface smoke: verify default values are sane ---

    #[test]
    fn cg_solver_options_default_has_sane_values() {
        let opts = CgSolverOptions::default();
        assert!(
            opts.tolerance > 0.0,
            "Default tolerance must be > 0.0, got {}",
            opts.tolerance
        );
        assert!(
            opts.max_iter > 0,
            "Default max_iter must be > 0, got {}",
            opts.max_iter
        );
    }

    // --- Contract panics ---

    /// `SolverMode::Parallel { threads: 0 }` must panic with a message
    /// naming `SolverMode::Parallel`.
    #[test]
    #[should_panic(expected = "Parallel")]
    fn parallel_zero_threads_panics() {
        let k = identity_1x1();
        let f = [1.0_f64];
        let opts = CgSolverOptions::default();
        let _ = solve_cg(&k, &f, opts, SolverMode::Parallel { threads: 0 });
    }

    /// `f.len() != k.nrows()` must panic with a descriptive message.
    #[test]
    #[should_panic(expected = "f.len()")]
    fn dimension_mismatch_f_len_panics() {
        let k = identity_2x2();
        let f = [1.0_f64]; // wrong length: 1 instead of 2
        let opts = CgSolverOptions::default();
        let _ = solve_cg(&k, &f, opts, SolverMode::Deterministic);
    }

    /// Non-square `k` must panic with a descriptive message.
    #[test]
    #[should_panic(expected = "square")]
    fn non_square_k_panics() {
        // Build a 2×3 matrix (non-square).
        let k: SparseRowMat<usize, f64> = SparseRowMat::try_new_from_triplets(
            2,
            3,
            &[
                Triplet::new(0_usize, 0_usize, 1.0_f64),
                Triplet::new(1_usize, 1_usize, 1.0_f64),
            ],
        )
        .unwrap();
        let f = [1.0_f64, 2.0_f64];
        let opts = CgSolverOptions::default();
        let _ = solve_cg(&k, &f, opts, SolverMode::Deterministic);
    }

    /// `opts.max_iter == 0` must panic with a message naming `max_iter`.
    #[test]
    #[should_panic(expected = "max_iter")]
    fn max_iter_zero_panics() {
        let k = identity_1x1();
        let f = [1.0_f64];
        let opts = CgSolverOptions {
            tolerance: 1e-8,
            max_iter: 0,
        };
        let _ = solve_cg(&k, &f, opts, SolverMode::Deterministic);
    }

    /// `opts.tolerance == 0.0` must panic with a message naming `tolerance`.
    #[test]
    #[should_panic(expected = "tolerance")]
    fn tolerance_zero_panics() {
        let k = identity_1x1();
        let f = [1.0_f64];
        let opts = CgSolverOptions {
            tolerance: 0.0,
            max_iter: 100,
        };
        let _ = solve_cg(&k, &f, opts, SolverMode::Deterministic);
    }

    /// `opts.tolerance < 0.0` must panic with a message naming `tolerance`.
    #[test]
    #[should_panic(expected = "tolerance")]
    fn tolerance_negative_panics() {
        let k = identity_1x1();
        let f = [1.0_f64];
        let opts = CgSolverOptions {
            tolerance: -1.0,
            max_iter: 100,
        };
        let _ = solve_cg(&k, &f, opts, SolverMode::Deterministic);
    }

    /// `opts.tolerance == f64::INFINITY` must panic: infinite tolerance makes
    /// the convergence check trivially satisfied on the first iteration.
    #[test]
    #[should_panic(expected = "tolerance")]
    fn tolerance_infinite_panics() {
        let k = identity_1x1();
        let f = [1.0_f64];
        let opts = CgSolverOptions {
            tolerance: f64::INFINITY,
            max_iter: 100,
        };
        let _ = solve_cg(&k, &f, opts, SolverMode::Deterministic);
    }

    /// `opts.tolerance == f64::NAN` must panic: NaN makes the convergence
    /// comparison undefined.
    #[test]
    #[should_panic(expected = "tolerance")]
    fn tolerance_nan_panics() {
        let k = identity_1x1();
        let f = [1.0_f64];
        let opts = CgSolverOptions {
            tolerance: f64::NAN,
            max_iter: 100,
        };
        let _ = solve_cg(&k, &f, opts, SolverMode::Deterministic);
    }

    // -----------------------------------------------------------------------
    // Step-3: identity-K trivial convergence
    // -----------------------------------------------------------------------

    /// For K = I₃ (3×3 identity), f = [1.0, 2.0, 3.0]:
    /// - Jacobi preconditioner M = diag(I) = I, so z₀ = f.
    /// - After one CG step: α₀ = (f·f) / (f·I·f) = 1.0, u₁ = f,
    ///   r₁ = f − I·f = 0. Convergence check trips at end of iter 0.
    /// - result.iterations == 1 (one iteration executed).
    /// - u == f bit-for-bit (identity Jacobi, no FP reordering).
    #[test]
    fn identity_k_converges_in_one_iter_deterministic() {
        let k = SparseRowMat::try_new_from_triplets(
            3,
            3,
            &[
                Triplet::new(0_usize, 0_usize, 1.0_f64),
                Triplet::new(1_usize, 1_usize, 1.0_f64),
                Triplet::new(2_usize, 2_usize, 1.0_f64),
            ],
        )
        .unwrap();
        let f = [1.0_f64, 2.0, 3.0];
        let opts = CgSolverOptions {
            tolerance: 1e-12,
            max_iter: 100,
        };
        let result = solve_cg(&k, &f, opts, SolverMode::Deterministic);

        assert!(result.converged, "identity K must converge");
        assert_eq!(
            result.iterations, 1,
            "identity K with Jacobi precond converges in exactly 1 iteration, got {}",
            result.iterations
        );
        for i in 0..3 {
            assert_eq!(
                result.u()[i].to_bits(),
                f[i].to_bits(),
                "u[{i}] = {} should be bit-equal to f[{i}] = {}",
                result.u()[i],
                f[i]
            );
        }
    }

    // -----------------------------------------------------------------------
    // Step-5: general SPD correctness
    // -----------------------------------------------------------------------

    /// K = [[4, 1], [1, 3]], f = [1, 2]. Analytical: u = (1/11, 7/11).
    /// CG on a 2×2 SPD converges in ≤ 2 iterations.
    #[test]
    fn hand_computed_2x2_spd_within_tolerance() {
        // Build K via triplets: symmetric 2×2.
        let k = SparseRowMat::try_new_from_triplets(
            2,
            2,
            &[
                Triplet::new(0_usize, 0_usize, 4.0_f64),
                Triplet::new(0_usize, 1_usize, 1.0_f64),
                Triplet::new(1_usize, 0_usize, 1.0_f64),
                Triplet::new(1_usize, 1_usize, 3.0_f64),
            ],
        )
        .unwrap();
        let f = [1.0_f64, 2.0];
        let opts = CgSolverOptions {
            tolerance: 1e-10,
            max_iter: 100,
        };
        let result = solve_cg(&k, &f, opts, SolverMode::Deterministic);

        assert!(result.converged, "2×2 SPD must converge");
        assert!(
            result.iterations <= 2,
            "CG converges in ≤ n iterations for n×n SPD; got {}",
            result.iterations
        );

        let u_expected = [1.0_f64 / 11.0, 7.0_f64 / 11.0];
        for i in 0..2 {
            let diff = (result.u()[i] - u_expected[i]).abs();
            assert!(
                diff < 1e-9,
                "u[{i}] = {} but expected {} (diff = {})",
                result.u()[i],
                u_expected[i],
                diff
            );
        }
    }

    // Fixture helpers shared by steps 5b, 11, 15.
    fn dimensionless_steel_like() -> crate::constitutive::IsotropicElastic {
        crate::constitutive::IsotropicElastic {
            youngs_modulus: 1.0,
            poisson_ratio: 0.3,
        }
    }

    const UNIT_TET_P1: [[f64; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ];

    /// Build the 4-tet fan-around-central-node-0 assembled K with Dirichlet
    /// pin on nodes 0 and 1 (DOFs 0..6), returning (K_spd, f) where f has
    /// a single non-zero entry on a free DOF.
    ///
    /// Same fixture as `assembly/global.rs::parallel_mode_tolerance_equivalent_to_deterministic_on_shared_dof_mesh`.
    /// n_nodes = 13, connectivity [0,1,2,3], [0,4,5,6], [0,7,8,9], [0,10,11,12].
    fn fan_mesh_k_spd_and_f() -> (faer::sparse::SparseRowMat<usize, f64>, Vec<f64>) {
        use crate::assembly::tet::element_stiffness_p1;
        use crate::assembly::{AssemblyElement, AssemblyMode, assemble_global_stiffness};
        use crate::boundary::{DirichletBc, apply_dirichlet_row_elimination};

        let mat = dimensionless_steel_like();
        let k_e = element_stiffness_p1(&UNIT_TET_P1, &mat);
        assert_eq!(k_e.n_dofs, 12);

        // 4 tets fanning around central node 0.
        let conns: [[usize; 4]; 4] = [[0, 1, 2, 3], [0, 4, 5, 6], [0, 7, 8, 9], [0, 10, 11, 12]];
        let n_nodes = 13;
        let elements: Vec<AssemblyElement<'_>> = conns
            .iter()
            .enumerate()
            .map(|(i, c)| AssemblyElement {
                id: i,
                connectivity: c,
                k_e: &k_e,
            })
            .collect();

        let mut k = assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Deterministic);

        // Pin DOFs 0..6 (nodes 0 and 1) to zero displacement. This removes
        // the rigid-body modes from the central node and one outer node,
        // making K SPD on the remaining free DOFs.
        let dim = 3 * n_nodes; // = 39
        let mut f = vec![0.0_f64; dim];
        let bcs: Vec<DirichletBc> = (0..6).map(|dof| DirichletBc { dof, value: 0.0 }).collect();
        apply_dirichlet_row_elimination(&mut k, &mut f, &bcs);

        // Apply a single non-zero load at a free DOF (DOF 6).
        f[6] = 1.0;

        (k, f)
    }

    // -----------------------------------------------------------------------
    // Step-7: max_iter exhaustion
    // -----------------------------------------------------------------------

    /// Use the 2×2 SPD problem with max_iter = 1 and impossibly tight
    /// tolerance. CG makes one step (which is insufficient for full
    /// convergence) and returns converged = false, iterations = 1.
    /// The solution vector u is non-zero (one step took effect).
    #[test]
    fn max_iter_exhaustion_returns_unconverged() {
        let k = SparseRowMat::try_new_from_triplets(
            2,
            2,
            &[
                Triplet::new(0_usize, 0_usize, 4.0_f64),
                Triplet::new(0_usize, 1_usize, 1.0_f64),
                Triplet::new(1_usize, 0_usize, 1.0_f64),
                Triplet::new(1_usize, 1_usize, 3.0_f64),
            ],
        )
        .unwrap();
        let f = [1.0_f64, 2.0];
        // max_iter = 1, impossibly tight tolerance → guaranteed non-convergence.
        let opts = CgSolverOptions {
            tolerance: 1e-15,
            max_iter: 1,
        };
        let result = solve_cg(&k, &f, opts, SolverMode::Deterministic);

        assert!(
            !result.converged,
            "must not converge with max_iter=1 and tol=1e-15"
        );
        assert_eq!(result.iterations, 1, "exactly the cap was consumed");
        assert_eq!(result.u().len(), 2, "u has the correct length");
        // At least one entry of u is non-zero (one CG step took effect).
        assert!(
            result.u().iter().any(|&v| v != 0.0),
            "u must be non-zero after one CG step: {:?}",
            result.u()
        );
    }

    // -----------------------------------------------------------------------
    // (step-8 impl — see commit message; plumbing already correct from step-4)
    // -----------------------------------------------------------------------

    /// Assembled fan-mesh K (after Dirichlet pin): solve_cg must converge and
    /// the residual ‖r‖ = ‖f − Ku‖ must be below 1e-9 · max(‖f‖, 1).
    #[test]
    fn assembled_fan_mesh_residual_below_tolerance() {
        let (k, f) = fan_mesh_k_spd_and_f();
        let n = f.len();
        let opts = CgSolverOptions {
            tolerance: 1e-10,
            max_iter: 1000,
        };
        let result = solve_cg(&k, &f, opts.clone(), SolverMode::Deterministic);

        assert!(
            result.converged,
            "fan-mesh CG must converge in {} iterations; got converged={}, iterations={}",
            opts.max_iter, result.converged, result.iterations
        );

        // Verify residual r = f − Ku using spmv_seq.
        let mut ku = vec![0.0_f64; n];
        spmv_seq(&k, result.u(), &mut ku);
        let mut residual = vec![0.0_f64; n];
        for i in 0..n {
            residual[i] = f[i] - ku[i];
        }
        let r_norm = norm2_squared(&residual).sqrt();
        let f_norm = norm2_squared(&f).sqrt();
        let tol = 1e-9 * f_norm.max(1.0);
        assert!(
            r_norm < tol,
            "residual ‖r‖ = {r_norm} ≥ tol = {tol} (‖f‖ = {f_norm})"
        );
    }

    // -----------------------------------------------------------------------
    // Step-9: zero-diagonal Jacobi panics
    // -----------------------------------------------------------------------

    /// Sub-case (a): K has an explicit zero stored at K[1][1].
    /// Both sub-cases share the "diagonal" substring in the panic message.
    #[test]
    #[should_panic(expected = "diagonal")]
    fn zero_diagonal_entry_panics() {
        // 3×3 matrix: K[0][0]=1, K[1][1]=0 (explicit zero), K[2][2]=1,
        // plus an off-diagonal to make it non-trivial.
        let k = SparseRowMat::try_new_from_triplets(
            3,
            3,
            &[
                Triplet::new(0_usize, 0_usize, 1.0_f64),
                Triplet::new(1_usize, 0_usize, 0.5_f64), // off-diagonal
                Triplet::new(1_usize, 1_usize, 0.0_f64), // explicit zero diagonal
                Triplet::new(2_usize, 2_usize, 1.0_f64),
            ],
        )
        .unwrap();
        let f = [1.0_f64, 2.0, 3.0];
        let opts = CgSolverOptions::default();
        let _ = solve_cg(&k, &f, opts, SolverMode::Deterministic);
    }

    /// Sub-case (b): K has no stored entry at K[1][1] at all (diagonal missing).
    #[test]
    #[should_panic(expected = "diagonal")]
    fn missing_diagonal_entry_panics() {
        // 3×3 matrix: K[0][0]=1, K[2][2]=1, K[1][1] not stored at all.
        let k = SparseRowMat::try_new_from_triplets(
            3,
            3,
            &[
                Triplet::new(0_usize, 0_usize, 1.0_f64),
                Triplet::new(1_usize, 0_usize, 0.5_f64), // off-diagonal only for row 1
                Triplet::new(2_usize, 2_usize, 1.0_f64),
            ],
        )
        .unwrap();
        let f = [1.0_f64, 2.0, 3.0];
        let opts = CgSolverOptions::default();
        let _ = solve_cg(&k, &f, opts, SolverMode::Deterministic);
    }

    // -----------------------------------------------------------------------
    // Step-11: deterministic back-to-back bit-stability
    // -----------------------------------------------------------------------

    /// Two consecutive Deterministic-mode calls on the same fan-mesh input
    /// must produce bit-identical outputs (u, iterations, converged).
    ///
    /// Mechanism: single-threaded + pairwise-tree reductions (fixed shape per
    /// input length) + slot-order vector ops → no scheduling dependence.
    ///
    /// This test pins the Deterministic-mode bit-stability contract as a
    /// regression guard (referenced in the solve_cg docstring).
    #[test]
    fn deterministic_back_to_back_bit_stable() {
        let (k, f) = fan_mesh_k_spd_and_f();
        let opts = CgSolverOptions {
            tolerance: 1e-10,
            max_iter: 1000,
        };

        let result_a = solve_cg(&k, &f, opts.clone(), SolverMode::Deterministic);
        let result_b = solve_cg(&k, &f, opts, SolverMode::Deterministic);

        assert_eq!(
            result_a.iterations, result_b.iterations,
            "iterations must be bit-stable"
        );
        assert_eq!(
            result_a.converged, result_b.converged,
            "converged flag must be bit-stable"
        );
        assert_eq!(result_a.u.len(), result_b.u.len(), "u lengths must match");
        for i in 0..result_a.u.len() {
            assert_eq!(
                result_a.u[i].to_bits(),
                result_b.u[i].to_bits(),
                "u[{i}] not bit-stable: a={} b={}",
                result_a.u[i],
                result_b.u[i]
            );
        }
    }

    // -----------------------------------------------------------------------
    // Step-15: parallel shared-DOF fan-mesh — tolerance-equivalence + back-to-back bit-stability
    // -----------------------------------------------------------------------

    /// Two assertions on the 4-tet fan-mesh assembled K with Dirichlet pin:
    ///
    /// (1) **Tolerance-equivalence**: `Parallel { threads: 4 }` vs `Deterministic`.
    ///     |u_par[i] − u_det[i]| < 1e-9 · max(1, |u_det[i]|) for every i.
    ///     The fan-mesh's central node 0 is shared by all 4 elements; rows 0–2
    ///     of K have many cross-element entries, so the parallel and deterministic
    ///     reductions use different tree shapes → slight FP delta is expected
    ///     (hence tolerance-equivalence, not bit-equality).
    ///
    ///     Note: n=39 < PAR_THRESHOLD (1024), so both modes take the sequential
    ///     path and the result is actually bit-identical in practice. The test
    ///     uses the looser tolerance-equivalence bound to remain valid for
    ///     n ≥ PAR_THRESHOLD where genuine parallel reduction is used.
    ///
    /// (2) **Fixed-thread back-to-back bit-stability**: two consecutive
    ///     `Parallel { threads: 4 }` calls produce bit-identical `u`,
    ///     `iterations`, `converged`. The chunk size `n.div_ceil(4)` is a
    ///     deterministic function of `(n, threads)` only; spawn/join order is
    ///     fixed; pairwise-tree shapes are fixed → no scheduling dependence.
    #[test]
    fn parallel_shared_dof_k_tolerance_equivalent_and_back_to_back_bit_stable() {
        let (k, f) = fan_mesh_k_spd_and_f();
        let opts = CgSolverOptions {
            tolerance: 1e-10,
            max_iter: 1000,
        };

        // (1) Tolerance-equivalence: Parallel { threads: 4 } vs Deterministic.
        let det = solve_cg(&k, &f, opts.clone(), SolverMode::Deterministic);
        assert!(det.converged, "deterministic must converge on fan-mesh");

        let par4 = solve_cg(&k, &f, opts.clone(), SolverMode::Parallel { threads: 4 });
        assert!(
            par4.converged,
            "Parallel {{ threads: 4 }} must converge on fan-mesh"
        );

        for i in 0..f.len() {
            let tol = 1e-9 * det.u()[i].abs().max(1.0);
            let diff = (par4.u()[i] - det.u()[i]).abs();
            assert!(
                diff < tol,
                "Tolerance-equivalence failure at i={i}: \
                 u_par={}, u_det={}, |diff|={diff} ≥ tol={tol}",
                par4.u()[i],
                det.u()[i],
            );
        }

        // (2) Back-to-back bit-stability for Parallel { threads: 4 }.
        let par4b = solve_cg(&k, &f, opts, SolverMode::Parallel { threads: 4 });
        assert_eq!(
            par4.iterations, par4b.iterations,
            "parallel back-to-back iterations must be bit-stable"
        );
        assert_eq!(
            par4.converged, par4b.converged,
            "parallel back-to-back converged flag must be bit-stable"
        );
        for i in 0..f.len() {
            assert_eq!(
                par4.u()[i].to_bits(),
                par4b.u()[i].to_bits(),
                "parallel back-to-back u[{i}] not bit-stable: a={} b={}",
                par4.u()[i],
                par4b.u()[i],
            );
        }
    }

    // -----------------------------------------------------------------------
    // Step-13: parallel disjoint-block-K bit-equal to deterministic
    // -----------------------------------------------------------------------

    /// Block-diagonal K of dimension 16 (four disjoint 4×4 SPD tridiagonal
    /// blocks). For t ∈ {1, 2, 4}: `Parallel { threads: t }` produces
    /// bit-identical result to `Deterministic`.
    ///
    /// **Why bit-equality holds here**: n=16 < PAR_THRESHOLD (1024), so all
    /// parallel helpers delegate to the sequential path — both modes use
    /// identical reduction trees and thus produce bit-identical results.
    ///
    /// **What bit-equality would require for n ≥ PAR_THRESHOLD**: Two
    /// conditions must both hold simultaneously:
    ///
    /// 1. *Partition-disjoint SpMV*: each row's SpMV reads only its own
    ///    block's `p`-slots. This ensures per-thread SpMV outputs are
    ///    independently computed with the same pairwise-tree shape as the
    ///    sequential path — no cross-thread accumulation in SpMV.
    ///
    /// 2. *Zero-contaminated dot products*: the f vector (and the derived
    ///    search directions `p`) must have enough zeros that the cross-chunk
    ///    additions in dot products always add zero as one operand. In this
    ///    test, f has exactly one nonzero per 4-element block, so inter-block
    ///    dot-product terms are zero — the different associativity groupings
    ///    across thread counts are masked. Changing f to a dense nonzero
    ///    vector would break bit-equality for t ≠ 1 at n ≥ PAR_THRESHOLD
    ///    (only tolerance-equivalence is guaranteed in that regime).
    #[test]
    fn parallel_disjoint_block_k_bit_equal_to_deterministic() {
        // Build a 16×16 block-diagonal SPD matrix. Four 4×4 tridiagonal blocks:
        // each block is [[4,1,0,0],[1,4,1,0],[0,1,4,1],[0,0,1,4]] (SPD: diag-dominant).
        let mut triplets: Vec<Triplet<usize, usize, f64>> = Vec::new();
        for block in 0..4_usize {
            let base = block * 4;
            for i in 0..4_usize {
                let row = base + i;
                triplets.push(Triplet::new(row, row, 4.0_f64)); // diagonal
                if i + 1 < 4 {
                    triplets.push(Triplet::new(row, row + 1, 1.0_f64)); // super-diagonal
                    triplets.push(Triplet::new(row + 1, row, 1.0_f64)); // sub-diagonal
                }
            }
        }
        let k = SparseRowMat::try_new_from_triplets(16, 16, &triplets).unwrap();

        // Non-zero f: one entry per block. This sparse structure is what allows
        // bit-equality across thread counts at n ≥ PAR_THRESHOLD (see test comment).
        let mut f = vec![0.0_f64; 16];
        f[0] = 1.0;
        f[4] = 2.0;
        f[8] = 3.0;
        f[12] = 4.0;

        let opts = CgSolverOptions {
            tolerance: 1e-10,
            max_iter: 500,
        };

        let det = solve_cg(&k, &f, opts.clone(), SolverMode::Deterministic);
        assert!(det.converged, "deterministic must converge");

        for &t in &[1_usize, 2, 4] {
            let par = solve_cg(&k, &f, opts.clone(), SolverMode::Parallel { threads: t });
            assert!(par.converged, "Parallel {{ threads: {t} }} must converge");
            assert_eq!(
                par.iterations, det.iterations,
                "Parallel {{ threads: {t} }} iterations ({}) ≠ Deterministic ({})",
                par.iterations, det.iterations
            );
            for i in 0..16 {
                assert_eq!(
                    par.u()[i].to_bits(),
                    det.u()[i].to_bits(),
                    "Parallel {{ threads: {t} }} u[{i}] = {} ≠ Deterministic u[{i}] = {}",
                    par.u()[i],
                    det.u()[i]
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // Task 2921: warm-state plumbing — initial-guess contract panic
    // -----------------------------------------------------------------------

    /// `solve_cg_warm` with an `initial_guess` whose length does not match
    /// `k.nrows()` must panic with a message naming `initial_guess`.
    /// Mirrors the existing `dimension_mismatch_f_len_panics` pattern.
    #[test]
    #[should_panic(expected = "initial_guess")]
    fn initial_guess_length_mismatch_panics() {
        // 2×2 SPD fixture (same as `hand_computed_2x2_spd_within_tolerance`).
        let k = SparseRowMat::try_new_from_triplets(
            2,
            2,
            &[
                Triplet::new(0_usize, 0_usize, 4.0_f64),
                Triplet::new(0_usize, 1_usize, 1.0_f64),
                Triplet::new(1_usize, 0_usize, 1.0_f64),
                Triplet::new(1_usize, 1_usize, 3.0_f64),
            ],
        )
        .unwrap();
        let f = [1.0_f64, 2.0];
        // initial_guess.len() = 3 but k.nrows() = 2 → must panic.
        let initial_guess = [1.0_f64, 2.0, 3.0];
        let opts = CgSolverOptions::default();
        let _ = solve_cg_warm(
            &k,
            &f,
            Some(&initial_guess),
            opts,
            SolverMode::Deterministic,
        );
    }

    // -----------------------------------------------------------------------
    // Task 2921: warm-state plumbing — iteration-reduction contract
    // -----------------------------------------------------------------------

    /// Warm-start with `u₁` (the cold solution to K·u = f₁) reduces the
    /// CG iteration count when re-solving K·u = f₂ for a perturbed
    /// `f₂ = f₁ + δ` with small δ — this is the iteration-reduction
    /// contract from PRD task #14.
    ///
    /// Procedure:
    /// 1. Solve K·u = f₁ cold → record `iter_cold_baseline` and `u₁`.
    /// 2. Solve K·u = f₂ cold → record `iter_cold_perturbed`.
    /// 3. Solve K·u = f₂ warm with `Some(&u₁)` → record `iter_warm_perturbed`.
    ///
    /// Asserts:
    /// - `iter_warm_perturbed < iter_cold_perturbed` (the contract).
    /// - `result_warm.converged == true`.
    /// - Tolerance-equivalence: warm and cold solutions of f₂ differ by
    ///   at most `1e-9 · max(1, |u_cold[i]|)` per component (both
    ///   converged to the same SPD system within tolerance).
    ///
    /// Uses `Deterministic` mode for bit-stability of intermediate solves.
    #[test]
    fn warm_start_with_perturbed_rhs_reduces_iteration_count() {
        let (k, f1) = fan_mesh_k_spd_and_f();
        let opts = CgSolverOptions {
            tolerance: 1e-10,
            max_iter: 1000,
        };

        // Cold solve K·u = f1.
        let cold_baseline = solve_cg(&k, &f1, opts.clone(), SolverMode::Deterministic);
        assert!(cold_baseline.converged, "cold baseline must converge");
        let u1 = cold_baseline.u.clone();

        // Build perturbed RHS f2 = f1 + δ. Pick a free DOF (DOF 7 — DOFs 0..6
        // are Dirichlet-pinned in this fixture) and add a small perturbation.
        let mut f2 = f1.clone();
        f2[7] += 1e-3;

        // Cold solve K·u = f2.
        let cold_perturbed = solve_cg(&k, &f2, opts.clone(), SolverMode::Deterministic);
        assert!(cold_perturbed.converged, "cold perturbed must converge");

        // Warm solve K·u = f2 with u1 as initial guess.
        let warm_perturbed = solve_cg_warm(&k, &f2, Some(&u1), opts, SolverMode::Deterministic);
        assert!(warm_perturbed.converged, "warm perturbed must converge");

        // Iteration-reduction contract.
        assert!(
            warm_perturbed.iterations < cold_perturbed.iterations,
            "warm ({}) must use fewer iterations than cold ({}) on perturbed RHS",
            warm_perturbed.iterations,
            cold_perturbed.iterations,
        );

        // Tolerance-equivalence: both solutions converged to the same SPD
        // system within tolerance, so they must agree component-wise to
        // within 1e-9 · max(1, |u_cold|).
        for i in 0..f2.len() {
            let tol = 1e-9 * cold_perturbed.u[i].abs().max(1.0);
            let diff = (warm_perturbed.u[i] - cold_perturbed.u[i]).abs();
            assert!(
                diff < tol,
                "tolerance-equivalence failure at i={i}: \
                 u_warm={}, u_cold={}, |diff|={diff} ≥ tol={tol}",
                warm_perturbed.u[i],
                cold_perturbed.u[i],
            );
        }
    }

    // -----------------------------------------------------------------------
    // Task 2921: warm-state plumbing — early-exit at exact solution
    // -----------------------------------------------------------------------

    /// `solve_cg_warm` with `initial_guess = Some(&u_exact)` (the cold-solve
    /// solution) must return `iterations = 0, converged = true`, with the
    /// returned `u` bit-equal to `u_exact` (no axpy ran).
    ///
    /// Why this matters: without the pre-loop early-exit, the first CG
    /// iteration would compute `α = rz / pkp` with both `rz ≈ 0` and
    /// `pkp ≈ 0`, producing 0/0 NaN that propagates through the result.
    /// The early-exit returns before the loop is entered — symmetric with
    /// the existing zero-RHS short-circuit at the dispatch site.
    #[test]
    fn warm_start_at_exact_solution_returns_in_zero_iterations() {
        // 2×2 SPD fixture: same as `hand_computed_2x2_spd_within_tolerance`.
        let k = SparseRowMat::try_new_from_triplets(
            2,
            2,
            &[
                Triplet::new(0_usize, 0_usize, 4.0_f64),
                Triplet::new(0_usize, 1_usize, 1.0_f64),
                Triplet::new(1_usize, 0_usize, 1.0_f64),
                Triplet::new(1_usize, 1_usize, 3.0_f64),
            ],
        )
        .unwrap();
        let f = [1.0_f64, 2.0];
        let opts = CgSolverOptions {
            tolerance: 1e-10,
            max_iter: 100,
        };

        // Cold-solve to obtain u_exact.
        let cold = solve_cg(&k, &f, opts.clone(), SolverMode::Deterministic);
        assert!(cold.converged, "cold solve must converge to obtain u_exact");
        let u_exact = cold.u.clone();

        // Warm-start at u_exact.
        let warm = solve_cg_warm(&k, &f, Some(&u_exact), opts, SolverMode::Deterministic);

        assert_eq!(
            warm.iterations, 0,
            "warm-start at exact solution must return in 0 iterations, got {}",
            warm.iterations
        );
        assert!(
            warm.converged,
            "warm-start at exact solution must report converged"
        );
        assert_eq!(
            warm.u.len(),
            u_exact.len(),
            "warm.u length must match u_exact"
        );
        for i in 0..u_exact.len() {
            assert_eq!(
                warm.u[i].to_bits(),
                u_exact[i].to_bits(),
                "warm.u[{i}] = {} must be bit-equal to u_exact[{i}] = {} \
                 (no axpy should have run)",
                warm.u[i],
                u_exact[i],
            );
        }
    }

    // -----------------------------------------------------------------------
    // Task 2921: warm-state plumbing — zero-RHS + non-zero initial guess
    // -----------------------------------------------------------------------

    /// Pin the dispatch-site contract that the zero-RHS short-circuit
    /// (`f_norm_sq == 0.0`) takes precedence over the caller-supplied
    /// `initial_guess`: the unique solution to `K·u = 0` for SPD `K` is
    /// `u = 0`, so honouring a non-zero guess and iterating would still
    /// converge to `u = 0`. Returning `vec![0.0; n]` directly avoids
    /// `0/0` in the relative-tolerance check `tol² · ‖f‖² == 0`.
    ///
    /// Without this pin a future refactor could plausibly choose to
    /// honour the guess (e.g. seed `u = u₀`, iterate until `‖r‖² < tol_sq`
    /// which is also 0 for any non-trivial guess) and silently change the
    /// short-circuit behaviour.
    #[test]
    fn zero_rhs_with_nonzero_initial_guess_still_returns_zero_u() {
        // 2×2 SPD fixture (same as `hand_computed_2x2_spd_within_tolerance`).
        let k = SparseRowMat::try_new_from_triplets(
            2,
            2,
            &[
                Triplet::new(0_usize, 0_usize, 4.0_f64),
                Triplet::new(0_usize, 1_usize, 1.0_f64),
                Triplet::new(1_usize, 0_usize, 1.0_f64),
                Triplet::new(1_usize, 1_usize, 3.0_f64),
            ],
        )
        .unwrap();
        let f = [0.0_f64, 0.0]; // zero RHS
        let initial_guess = [1.0_f64, 1.0]; // non-zero guess
        let opts = CgSolverOptions {
            tolerance: 1e-10,
            max_iter: 100,
        };

        let result = solve_cg_warm(
            &k,
            &f,
            Some(&initial_guess),
            opts,
            SolverMode::Deterministic,
        );

        assert_eq!(
            result.iterations, 0,
            "zero-RHS short-circuit must return in 0 iterations regardless of \
             initial_guess; got {}",
            result.iterations,
        );
        assert!(
            result.converged,
            "zero-RHS short-circuit must report converged",
        );
        assert_eq!(result.u(), &[0.0_f64, 0.0_f64], "u must be the zero vector");
    }

    // -----------------------------------------------------------------------
    // Task 2921: warm-state plumbing — `solve_cg_warm` None-shim equivalence
    // -----------------------------------------------------------------------

    /// `solve_cg_warm(&k, &f, None, opts, mode)` must produce a `CgResult`
    /// bit-equal to `solve_cg(&k, &f, opts, mode)` on the same input.
    ///
    /// This is the backward-compatibility contract: the `None` initial-guess
    /// branch in `solve_cg_warm` must take the exact same code path as the
    /// existing `solve_cg` (u₀ = 0, r₀ = f) so every existing caller and test
    /// of `solve_cg` continues to produce bit-identical output. The
    /// fan-mesh fixture exercises a non-trivial CG iteration count.
    ///
    /// Both `Deterministic` and `Parallel { threads: 4 }` modes are pinned —
    /// the determinism contract is per-mode, so we verify the None-shim
    /// preserves bit-equality in each.
    #[test]
    fn solve_cg_warm_with_none_matches_solve_cg_bit_for_bit() {
        let (k, f) = fan_mesh_k_spd_and_f();
        let opts = CgSolverOptions {
            tolerance: 1e-10,
            max_iter: 1000,
        };

        // Deterministic mode.
        let det_cold = solve_cg(&k, &f, opts.clone(), SolverMode::Deterministic);
        let det_warm_none = solve_cg_warm(&k, &f, None, opts.clone(), SolverMode::Deterministic);
        assert_eq!(
            det_cold.iterations, det_warm_none.iterations,
            "Deterministic: solve_cg_warm(None) iterations must match solve_cg"
        );
        assert_eq!(
            det_cold.converged, det_warm_none.converged,
            "Deterministic: solve_cg_warm(None) converged must match solve_cg"
        );
        assert_eq!(
            det_cold.u.len(),
            det_warm_none.u.len(),
            "Deterministic: u lengths must match"
        );
        for i in 0..det_cold.u.len() {
            assert_eq!(
                det_cold.u[i].to_bits(),
                det_warm_none.u[i].to_bits(),
                "Deterministic: solve_cg_warm(None) u[{i}] = {} ≠ solve_cg u[{i}] = {}",
                det_warm_none.u[i],
                det_cold.u[i],
            );
        }

        // Parallel { threads: 4 } mode.
        let par_cold = solve_cg(&k, &f, opts.clone(), SolverMode::Parallel { threads: 4 });
        let par_warm_none = solve_cg_warm(
            &k,
            &f,
            None,
            opts.clone(),
            SolverMode::Parallel { threads: 4 },
        );
        assert_eq!(
            par_cold.iterations, par_warm_none.iterations,
            "Parallel: solve_cg_warm(None) iterations must match solve_cg"
        );
        assert_eq!(
            par_cold.converged, par_warm_none.converged,
            "Parallel: solve_cg_warm(None) converged must match solve_cg"
        );
        for i in 0..par_cold.u.len() {
            assert_eq!(
                par_cold.u[i].to_bits(),
                par_warm_none.u[i].to_bits(),
                "Parallel: solve_cg_warm(None) u[{i}] = {} ≠ solve_cg u[{i}] = {}",
                par_warm_none.u[i],
                par_cold.u[i],
            );
        }
    }

    /// Pins the `build_initial_u_r` `Some` branch: asserts that the returned
    /// residual `r` equals `f − K·u₀` to bit precision, verified against a
    /// hand-computed oracle.  K = [[4,1],[1,3]], u₀ = [0.5, 0.25],
    /// f = [1.0, 2.0] → K·u₀ = [2.25, 1.25] → r = [−1.25, 0.75].
    /// Using literal expected values (not spmv_seq-derived) makes this a true
    /// contract pin rather than a self-equality check.
    #[test]
    fn build_initial_u_r_some_branch_residual_equals_f_minus_k_u0() {
        // 2×2 SPD fixture: K = [[4.0, 1.0], [1.0, 3.0]].
        let k = SparseRowMat::try_new_from_triplets(
            2,
            2,
            &[
                Triplet::new(0_usize, 0_usize, 4.0_f64),
                Triplet::new(0_usize, 1_usize, 1.0_f64),
                Triplet::new(1_usize, 0_usize, 1.0_f64),
                Triplet::new(1_usize, 1_usize, 3.0_f64),
            ],
        )
        .unwrap();
        let u_0 = [0.5_f64, 0.25];
        let f = [1.0_f64, 2.0];

        // K·u₀ = [4·0.5+1·0.25, 1·0.5+3·0.25] = [2.25, 1.25]
        // r = f − K·u₀ = [1.0−2.25, 2.0−1.25] = [−1.25, 0.75]
        let expected_r = [-1.25_f64, 0.75_f64];

        // Call the helper under test.
        let (u, r) = build_initial_u_r(&f, Some(&u_0), |p, out| spmv_seq(&k, p, out));

        // u must be a copy of u₀.
        assert_eq!(u.len(), 2, "u.len() must equal 2");
        for i in 0..2 {
            assert_eq!(
                u[i].to_bits(),
                u_0[i].to_bits(),
                "u[{i}] = {} ≠ u_0[{i}] = {}",
                u[i],
                u_0[i],
            );
        }

        // r must equal f − K·u₀ to bit precision, verified against literal oracle.
        assert_eq!(r.len(), 2, "r.len() must equal 2");
        for i in 0..2 {
            assert_eq!(
                r[i].to_bits(),
                expected_r[i].to_bits(),
                "r[{i}] = {} ≠ expected_r[{i}] = {}",
                r[i],
                expected_r[i],
            );
        }
    }

    /// Pins the `pairwise_tree_sum_fn(start, len, get)` signature contract:
    /// exercises every base-case arm with start=0, the start>0 offset path,
    /// and a recursion-triggering len>8 with both start=0 and start>0.
    /// A tree-shape bit-pin asserts that len=12 (mid=6) produces bits matching
    /// `((xs[0]+xs[1]+xs[2]) + (xs[3]+xs[4]+xs[5])) + ((xs[6]+xs[7]+xs[8]) + (xs[9]+xs[10]+xs[11]))`,
    /// confirming the mid=6 split and base-case-6 arithmetic order.
    #[test]
    fn pairwise_tree_sum_fn_with_start_offset_pins_contract() {
        // Empty case.
        assert_eq!(pairwise_tree_sum_fn(0, 0, &|_| 0.0), 0.0);

        // Singleton.
        assert_eq!(pairwise_tree_sum_fn(0, 1, &|i| (i as f64) + 1.0), 1.0);

        // Lengths 2..=8, start=0: get(i) = i+1, so sum = 1+2+…+n = n*(n+1)/2.
        // Exercises every base-case arm (len 2, 3, 4, 5, 6, 7, 8).
        for n in 2..=8_usize {
            let expected = (n * (n + 1) / 2) as f64;
            let actual = pairwise_tree_sum_fn(0, n, &|i| (i as f64) + 1.0);
            assert_eq!(
                actual, expected,
                "start=0, len={n}: expected {expected}, got {actual}"
            );
        }

        // start > 0: pairwise_tree_sum_fn(2, 3, get) gives get(2)+get(3)+get(4)
        //            = 3.0 + 4.0 + 5.0 = 12.0 (using get(i) = i+1).
        assert_eq!(pairwise_tree_sum_fn(2, 3, &|i| (i as f64) + 1.0), 12.0);

        // len > 8, start=0: sum 1..=16 = 16*17/2 = 136.
        assert_eq!(pairwise_tree_sum_fn(0, 16, &|i| (i + 1) as f64), 136.0);

        // len > 8, start > 0: pairwise_tree_sum_fn(5, 16, get) gives sum 6..=21
        //   = (6+21)*16/2 = 216.
        assert_eq!(pairwise_tree_sum_fn(5, 16, &|i| (i + 1) as f64), 216.0);

        // Tree-shape bit-pin: len=12 → mid=6, so two base-case-6 arms fire.
        // Prime reciprocals are not exact in f64, so different addition groupings
        // produce different bit patterns — a left-fold or a reordered split would
        // fail this check. expected_bits mirrors the exact pairwise-tree expansion:
        //   ((xs[0]+xs[1]+xs[2]) + (xs[3]+xs[4]+xs[5]))
        //   + ((xs[6]+xs[7]+xs[8]) + (xs[9]+xs[10]+xs[11]))
        let xs: [f64; 12] = [
            1.0_f64 / 3.0,
            1.0_f64 / 7.0,
            1.0_f64 / 11.0,
            1.0_f64 / 13.0,
            1.0_f64 / 17.0,
            1.0_f64 / 19.0,
            1.0_f64 / 23.0,
            1.0_f64 / 29.0,
            1.0_f64 / 31.0,
            1.0_f64 / 37.0,
            1.0_f64 / 41.0,
            1.0_f64 / 43.0,
        ];
        let expected_bits = (((xs[0] + xs[1] + xs[2]) + (xs[3] + xs[4] + xs[5]))
            + ((xs[6] + xs[7] + xs[8]) + (xs[9] + xs[10] + xs[11])))
            .to_bits();
        assert_eq!(
            pairwise_tree_sum_fn(0, 12, &|i| xs[i]).to_bits(),
            expected_bits,
            "tree-shape pin: mid=6 split must produce bits matching explicit pairwise grouping"
        );
    }

    // -----------------------------------------------------------------------
    // GR-016 ζ step-1/3: solve_cg_with_progress callback tests
    // -----------------------------------------------------------------------

    /// `solve_cg_with_progress` fires the callback exactly once per CG
    /// iteration, and the callback receives monotonically increasing iter
    /// indices starting at 1 and finite positive residuals.
    ///
    /// Uses the fan-mesh fixture (33 free DOFs, takes ~tens of iterations)
    /// so the callback receives a non-trivial sequence.
    #[test]
    fn solve_cg_with_progress_fires_callback_per_iteration_and_converges() {
        let (k, f) = fan_mesh_k_spd_and_f();
        let opts = CgSolverOptions {
            tolerance: 1e-10,
            max_iter: 1000,
        };

        let mut observed: Vec<(usize, f64)> = Vec::new();
        let result = solve_cg_with_progress(
            &k,
            &f,
            None,
            opts,
            SolverMode::Deterministic,
            &mut |iter, residual| {
                observed.push((iter, residual));
                CgIterationControl::Continue
            },
        );

        // (a) The solve must converge on this SPD problem.
        assert!(
            result.converged,
            "solve_cg_with_progress must converge on fan-mesh; iterations={}",
            result.iterations
        );

        // (b) Callback fires exactly once per iteration.
        assert_eq!(
            observed.len(),
            result.iterations,
            "callback invocation count ({}) must equal result.iterations ({})",
            observed.len(),
            result.iterations
        );

        // (c) Iter values are strictly monotonically increasing starting at 1.
        for (idx, &(iter, _)) in observed.iter().enumerate() {
            assert_eq!(
                iter,
                idx + 1,
                "expected iter={} at position {}, got {}",
                idx + 1,
                idx,
                iter
            );
        }

        // (d) Every observed residual is finite and non-negative.
        //
        // Note: residual CAN be exactly 0.0 if the problem converges with zero
        // maintained residual (e.g. the fan-mesh with a single-entry RHS converges
        // in 1 step with α=1 exactly, driving r₁ = 0 due to DOF decoupling).
        // The L2 norm of a residual vector is mathematically non-negative; we
        // assert finite+non-negative rather than strictly positive. Escalation
        // esc-3543-111 documents the root cause analysis.
        for &(iter, residual) in &observed {
            assert!(
                residual.is_finite() && residual >= 0.0,
                "residual at iter={iter} must be finite and non-negative, got {residual}"
            );
        }
    }

    /// Returning `CgIterationControl::Cancel` from the progress callback stops
    /// the CG loop immediately and sets `converged = false`, even when the
    /// residual would have satisfied the convergence criterion.
    ///
    /// The fan-mesh with a single-entry RHS converges in exactly 1 CG iteration
    /// (α=1 exactly drives r₁ → 0, as documented by the esc-3543-111 analysis
    /// in `solve_cg_with_progress_fires_callback_per_iteration_and_converges`).
    /// We cancel at `iter == 1`: the Cancel check in `cg_loop` runs BEFORE the
    /// convergence check, so the convergence check is never reached and the
    /// solver returns `converged = false` rather than `converged = true`.
    ///
    /// Assertions:
    /// (a) `result.converged == false`  — Cancel is not convergence; the
    ///     `converged = true` path is unreachable once Cancel is checked first.
    ///     Against step-2 (Cancel ignored), the solver converges normally and
    ///     returns `converged = true` → **RED**.
    /// (b) `result.iterations == 1`     — one iteration executed.
    /// (c) callback invoked exactly once — no re-entry after Cancel.
    ///
    /// This test is RED against step-2's implementation because `cg_loop`
    /// ignores the Cancel return value in that step; step-4 adds the
    /// cooperative-cancellation branch (Cancel check before convergence check).
    #[test]
    fn solve_cg_with_progress_cancel_terminates_iteration_within_one_step() {
        let (k, f) = fan_mesh_k_spd_and_f();
        // Tight tolerance to ensure the fan-mesh would converge at iter=1
        // without Cancel (the problem is trivially solvable in one CG step).
        // max_iter=1000 ensures the loop would run if Cancel is ignored.
        let opts = CgSolverOptions {
            tolerance: 1e-10,
            max_iter: 1000,
        };

        let mut call_count: usize = 0;
        let result = solve_cg_with_progress(
            &k,
            &f,
            None,
            opts,
            SolverMode::Deterministic,
            &mut |iter, _residual| {
                call_count += 1;
                // Cancel on the first iteration — the convergence check at
                // iter==1 would otherwise succeed; Cancel must win.
                if iter == 1 {
                    CgIterationControl::Cancel
                } else {
                    CgIterationControl::Continue
                }
            },
        );

        // (a) Cancel is not convergence — the convergence path is unreachable
        // once cg_loop checks Cancel before the convergence branch.
        // Against step-2 the solver ignores Cancel and returns converged=true
        // (the residual satisfies tolerance at iter=1). → RED.
        assert!(
            !result.converged,
            "result.converged must be false when Cancel was returned at iter=1; \
             got converged=true (step-4 Cancel-before-convergence branch not wired)"
        );

        // (b) Loop terminated exactly at the Cancel iteration.
        assert_eq!(
            result.iterations,
            1,
            "result.iterations must be 1 (the Cancel iteration); got {}",
            result.iterations
        );

        // (c) Callback invoked exactly once — no re-entry after Cancel.
        assert_eq!(
            call_count,
            1,
            "callback must be invoked exactly once (stopped at Cancel); \
             got {call_count} invocations"
        );
    }

    /// `solve_cg_with_progress` fires the callback across **multiple** CG
    /// iterations on a coupled system that cannot converge in a single step.
    ///
    /// The two tests above both use the fan-mesh fixture, which converges in
    /// exactly 1 CG iteration (the mesh DOFs are decoupled after Dirichlet
    /// elimination, so α = 1 drives r₁ → 0 exactly). That means neither
    /// test exercises `observed[idx]` for `idx > 0`, leaving the 1-indexed
    /// iter-counter contract and residual-sequence unverified across multiple
    /// iterations.
    ///
    /// This test uses a 3×3 tridiagonal SPD system with off-diagonal coupling:
    ///
    /// ```text
    ///     ⎡  4  −1   0 ⎤       ⎡ 1 ⎤
    /// K = ⎢ −1   4  −1 ⎥   f = ⎢ 2 ⎥
    ///     ⎣  0  −1   4 ⎦       ⎣ 3 ⎦
    /// ```
    ///
    /// Eigenvalues of K are 4 ± √2 and 4 (all strictly positive → K is SPD).
    /// With the asymmetric RHS [1, 2, 3] and Jacobi preconditioner the solver
    /// requires > 1 step before residual drops below 1e-12.
    ///
    /// Assertions:
    /// (a) Converges on this SPD system.
    /// (b) `observed.len() > 1` — the multi-step contract is actually exercised.
    /// (c) Iter values are exactly 1-indexed: `observed[i].0 == i + 1`.
    /// (d) Every residual is finite and non-negative.
    #[test]
    fn solve_cg_with_progress_multi_iteration_callback_sequence() {
        let k = SparseRowMat::try_new_from_triplets(
            3,
            3,
            &[
                Triplet::new(0_usize, 0_usize, 4.0_f64),
                Triplet::new(0_usize, 1_usize, -1.0_f64),
                Triplet::new(1_usize, 0_usize, -1.0_f64),
                Triplet::new(1_usize, 1_usize, 4.0_f64),
                Triplet::new(1_usize, 2_usize, -1.0_f64),
                Triplet::new(2_usize, 1_usize, -1.0_f64),
                Triplet::new(2_usize, 2_usize, 4.0_f64),
            ],
        )
        .unwrap();
        let f = [1.0_f64, 2.0_f64, 3.0_f64];
        let opts = CgSolverOptions {
            tolerance: 1e-12,
            max_iter: 100,
        };

        let mut observed: Vec<(usize, f64)> = Vec::new();
        let result = solve_cg_with_progress(
            &k,
            &f,
            None,
            opts,
            SolverMode::Deterministic,
            &mut |iter, residual| {
                observed.push((iter, residual));
                CgIterationControl::Continue
            },
        );

        // (a) Must converge on this SPD system.
        assert!(
            result.converged,
            "solve_cg_with_progress must converge on tridiagonal SPD system; iterations={}",
            result.iterations
        );

        // (b) The coupled system requires more than one CG iteration — this is
        // the invariant that the fan-mesh tests cannot pin.
        assert!(
            observed.len() > 1,
            "expected > 1 callback invocation on coupled tridiagonal system, got {}; \
             the multi-iteration iter-counter contract was not exercised",
            observed.len()
        );

        // (c) Iter values must be exactly 1-indexed with no gaps.
        for (idx, &(iter, _)) in observed.iter().enumerate() {
            assert_eq!(
                iter,
                idx + 1,
                "expected iter={} at position {idx}, got {iter}",
                idx + 1
            );
        }

        // (d) Every residual must be finite and non-negative.
        for &(iter, residual) in &observed {
            assert!(
                residual.is_finite() && residual >= 0.0,
                "residual at iter={iter} must be finite and non-negative, got {residual}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Accessor API tests (task-3366: encapsulate CgResult.u)
    // -----------------------------------------------------------------------

    /// `CgResult::u()` returns a slice identical to the solution.
    ///
    /// Uses the 3×3 identity-K fixture where u == f bit-exactly (same fixture
    /// as `identity_k_converges_in_one_iter_deterministic`).
    #[test]
    fn cg_result_u_accessor_returns_solution_slice() {
        let k = SparseRowMat::try_new_from_triplets(
            3,
            3,
            &[
                Triplet::new(0_usize, 0_usize, 1.0_f64),
                Triplet::new(1_usize, 1_usize, 1.0_f64),
                Triplet::new(2_usize, 2_usize, 1.0_f64),
            ],
        )
        .unwrap();
        let f = [1.0_f64, 2.0, 3.0];
        let opts = CgSolverOptions {
            tolerance: 1e-12,
            max_iter: 100,
        };
        let result = solve_cg(&k, &f, opts, SolverMode::Deterministic);

        assert!(result.converged, "identity K must converge");
        // u() must return a slice with the same length and bit-identical values.
        assert_eq!(
            result.u().len(),
            f.len(),
            "u() slice length must equal f.len()"
        );
        assert_eq!(
            result.u(),
            f.as_slice(),
            "identity-K solution u() must equal f bit-exactly"
        );
    }

    /// `CgResult::shared_u()` and `into_shared_u()` both hand back the SAME
    /// underlying allocation without copying.
    ///
    /// Uses the 2×2 SPD fixture (K=[[4,1],[1,3]], f=[1,2]) — same matrix as
    /// `hand_computed_2x2_spd_within_tolerance`.
    ///
    /// Assertions:
    /// (a) Two `shared_u()` calls produce `Arc` handles that `ptr_eq` each other
    ///     — they both point at the same allocation.
    /// (b) `h1.as_slice() == result.u()` — the shared handle's data matches the
    ///     read accessor.
    /// (c) `into_shared_u()` (consuming) also returns the SAME underlying
    ///     allocation as the earlier `shared_u()` handles (zero-copy donation).
    #[test]
    fn cg_result_donation_accessors_share_allocation_without_copy() {
        use std::sync::Arc;
        let k = SparseRowMat::try_new_from_triplets(
            2,
            2,
            &[
                Triplet::new(0_usize, 0_usize, 4.0_f64),
                Triplet::new(0_usize, 1_usize, 1.0_f64),
                Triplet::new(1_usize, 0_usize, 1.0_f64),
                Triplet::new(1_usize, 1_usize, 3.0_f64),
            ],
        )
        .unwrap();
        let f = [1.0_f64, 2.0];
        let opts = CgSolverOptions {
            tolerance: 1e-10,
            max_iter: 100,
        };
        let result = solve_cg(&k, &f, opts, SolverMode::Deterministic);
        assert!(result.converged, "2×2 SPD must converge");

        // (a) Two shared_u() calls must return Arc handles to the SAME allocation.
        let h1 = result.shared_u();
        let h2 = result.shared_u();
        assert!(
            Arc::ptr_eq(&h1, &h2),
            "shared_u() must return handles to the same Arc allocation"
        );

        // (b) Shared handle content matches the read accessor.
        assert_eq!(
            h1.as_slice(),
            result.u(),
            "shared_u() content must equal u() slice"
        );

        // (c) into_shared_u() (consuming) also hands back the SAME allocation.
        let owned = result.into_shared_u();
        assert!(
            Arc::ptr_eq(&h1, &owned),
            "into_shared_u() must return the same underlying Arc (zero-copy donation)"
        );
    }
}
