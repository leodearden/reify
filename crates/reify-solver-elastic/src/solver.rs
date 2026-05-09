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
/// (a) Chunk size `n.div_ceil(threads).max(1)` is a deterministic function of
///     `(n, threads)` only — no work-stealing or load-balancing.
/// (b) Threads spawn sequentially in chunk-iteration order; handle slot `t`
///     always corresponds to the worker for chunk `t`.
/// (c) Worker handles are joined in spawn order (t-ascending), and the
///     cross-thread combine runs `pairwise_tree_sum` over the spawn-ordered
///     partial-sums Vec.
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
    /// Must be `> 0`. Default: `1e-8`.
    pub tolerance: f64,
    /// Maximum number of CG iterations before giving up.
    /// Default: `1000`.
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
/// - On convergence: the iteration on which the residual met the tolerance.
/// - On cap-out: `iterations == max_iter` (the budget was exhausted).
/// - On zero RHS `‖f‖ == 0`: `iterations == 0` (trivial exact solution).
#[derive(Debug, Clone, PartialEq)]
pub struct CgResult {
    /// Solution vector `u` of length `k.nrows()`.
    pub u: Vec<f64>,
    /// Number of CG iterations executed.
    pub iterations: usize,
    /// `true` if the residual met the tolerance criterion before `max_iter`.
    pub converged: bool,
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
/// - Single-threaded execution ⟹ no thread-scheduling order dependence.
/// - Pairwise-tree reductions have a tree shape that is a deterministic
///   function of input length only ⟹ no scheduler-dependent reduction order.
/// - All vector ops iterate slot `0 → n−1` in slice order ⟹ no
///   iteration-order dependence.
///
/// The `deterministic_back_to_back_bit_stable` test pins this contract as a
/// regression guard: identical inputs produce bit-for-bit identical outputs
/// on the same machine and across machines.
///
/// # Panics
///
/// - `SolverMode::Parallel { threads: 0 }` — auto-fallback would silently
///   mask caller bugs; the panic surfaces them at the call site.
/// - `f.len() != k.nrows()` — the RHS vector must be sized to the system.
/// - `k.nrows() != k.ncols()` — `K` must be square.
/// - Any row `i` of `K` has no stored diagonal entry or has `K[i][i] == 0.0`
///   (Jacobi preconditioner is undefined without a non-zero diagonal).
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

    // Placeholder: real CG loop is implemented in step-4.
    // The contract asserts above already handle step-1's panic tests.
    let n = f.len();
    CgResult {
        u: vec![0.0; n],
        iterations: 0,
        converged: false,
    }
}

#[cfg(test)]
mod tests {
    use super::{CgResult, CgSolverOptions, SolverMode, solve_cg};
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

    // --- Public-surface smoke: construct each type ---

    #[test]
    fn solver_mode_deterministic_is_constructible() {
        let _mode = SolverMode::Deterministic;
    }

    #[test]
    fn solver_mode_parallel_is_constructible() {
        let _mode = SolverMode::Parallel { threads: 2 };
    }

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

    #[test]
    fn cg_result_fields_are_accessible() {
        // Construct a CgResult directly to verify the public fields exist.
        let r = CgResult {
            u: vec![1.0, 2.0],
            iterations: 5,
            converged: true,
        };
        assert_eq!(r.u.len(), 2);
        assert_eq!(r.iterations, 5);
        assert!(r.converged);
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
                result.u[i].to_bits(),
                f[i].to_bits(),
                "u[{i}] = {} should be bit-equal to f[{i}] = {}",
                result.u[i],
                f[i]
            );
        }
    }
}
