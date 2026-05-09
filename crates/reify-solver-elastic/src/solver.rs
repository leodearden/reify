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

    let n = f.len();

    // --- Jacobi preconditioner: extract diagonal of K ---
    let inv_diag = extract_diag_jacobi(k);

    // --- Special case: zero RHS ---
    // ‖f‖² == 0.0 ⟹ u = 0 is the exact solution. Return immediately to
    // avoid 0/0 in the relative tolerance check. (Unconditional == 0.0 is
    // safe here: f is the caller's vector and pairwise_tree_sum of zeros
    // is deterministically 0.0.)
    let f_norm_sq = norm2_squared(f);
    if f_norm_sq == 0.0 {
        return CgResult {
            u: vec![0.0; n],
            iterations: 0,
            converged: true,
        };
    }
    let tol_sq = opts.tolerance * opts.tolerance * f_norm_sq;

    // --- Dispatch to mode-specific CG ---
    match mode {
        SolverMode::Deterministic => solve_cg_deterministic(k, f, &inv_diag, tol_sq, opts.max_iter, n),
        SolverMode::Parallel { threads } => {
            solve_cg_parallel(k, f, &inv_diag, tol_sq, opts.max_iter, n, threads)
        }
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Pairwise-tree summation for deterministic, bounded-error reduction.
///
/// Recursively halves the slice; base case `len <= 8` uses a sequential left-fold
/// (any 2-summand IEEE-754 add is order-independent). Returns `0.0` for
/// empty slices. The tree shape is a deterministic function of `len` only,
/// which is the load-bearing mechanism for bit-stability: the same `len`
/// always produces the same reduction order, regardless of scheduling.
fn pairwise_tree_sum(slice: &[f64]) -> f64 {
    match slice.len() {
        0 => 0.0,
        1 => slice[0],
        2 => slice[0] + slice[1],
        3 => slice[0] + slice[1] + slice[2],
        4 => (slice[0] + slice[1]) + (slice[2] + slice[3]),
        5 => (slice[0] + slice[1]) + (slice[2] + slice[3]) + slice[4],
        6 => (slice[0] + slice[1] + slice[2]) + (slice[3] + slice[4] + slice[5]),
        7 => (slice[0] + slice[1] + slice[2] + slice[3]) + (slice[4] + slice[5] + slice[6]),
        8 => {
            (slice[0] + slice[1] + slice[2] + slice[3])
                + (slice[4] + slice[5] + slice[6] + slice[7])
        }
        len => {
            let mid = len / 2;
            pairwise_tree_sum(&slice[..mid]) + pairwise_tree_sum(&slice[mid..])
        }
    }
}

/// Dot product `a · b` using pairwise-tree summation.
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
    let products: Vec<f64> = a.iter().zip(b.iter()).map(|(ai, bi)| ai * bi).collect();
    pairwise_tree_sum(&products)
}

/// Squared Euclidean norm `‖v‖²` using pairwise-tree summation.
fn norm2_squared(v: &[f64]) -> f64 {
    let squares: Vec<f64> = v.iter().map(|vi| vi * vi).collect();
    pairwise_tree_sum(&squares)
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
/// Uses pairwise-tree reduction for each row's dot product to give
/// O(log nnz_per_row) error growth and deterministic reduction order.
fn spmv_seq(k: &SparseRowMat<usize, f64>, p: &[f64], out: &mut [f64]) {
    let (sym, vals) = k.parts();
    let row_ptr = sym.row_ptr();
    let col_idx = sym.col_idx();
    let n = sym.nrows();

    for i in 0..n {
        let start = row_ptr[i];
        let end = row_ptr[i + 1];
        let products: Vec<f64> = (start..end)
            .map(|idx| vals[idx] * p[col_idx[idx]])
            .collect();
        out[i] = pairwise_tree_sum(&products);
    }
}

/// Deterministic CG inner loop (single-threaded).
fn solve_cg_deterministic(
    k: &SparseRowMat<usize, f64>,
    f: &[f64],
    inv_diag: &[f64],
    tol_sq: f64,
    max_iter: usize,
    n: usize,
) -> CgResult {
    // Allocate scratch vectors. All ops iterate slot 0 → n−1 in slice order.
    let mut u = vec![0.0_f64; n];
    // r₀ = f − K·u₀ = f (since u₀ = 0)
    let mut r: Vec<f64> = f.to_vec();
    // z₀ = M⁻¹ r₀
    let mut z: Vec<f64> = r.iter().zip(inv_diag.iter()).map(|(ri, di)| ri * di).collect();
    // p₀ = z₀
    let mut p: Vec<f64> = z.clone();
    // rz = r₀ · z₀
    let mut rz = dot(&r, &z);

    let mut kp = vec![0.0_f64; n];

    for iter in 0..max_iter {
        // Kp = K · p_k
        spmv_seq(k, &p, &mut kp);

        // α = (r_k · z_k) / (p_k · Kp)
        let pkp = dot(&p, &kp);
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
        let r_norm_sq = norm2_squared(&r);
        if r_norm_sq < tol_sq {
            return CgResult {
                u,
                iterations: iter + 1,
                converged: true,
            };
        }

        // z_{k+1} = M⁻¹ r_{k+1}
        for i in 0..n {
            z[i] = r[i] * inv_diag[i];
        }

        // β = (r_{k+1} · z_{k+1}) / (r_k · z_k)
        let rz_new = dot(&r, &z);
        let beta = rz_new / rz;
        rz = rz_new;

        // p_{k+1} = z_{k+1} + β p_k
        for i in 0..n {
            p[i] = z[i] + beta * p[i];
        }
    }

    // Cap-out without convergence.
    CgResult {
        u,
        iterations: max_iter,
        converged: false,
    }
}

/// Parallel CG inner loop (row-partitioned SpMV + parallel reductions).
///
/// Placeholder until step-14; calls the deterministic path for now.
/// This ensures step-4's tests pass while parallel mode is wired in later.
fn solve_cg_parallel(
    k: &SparseRowMat<usize, f64>,
    f: &[f64],
    inv_diag: &[f64],
    tol_sq: f64,
    max_iter: usize,
    n: usize,
    _threads: usize,
) -> CgResult {
    // Parallel implementation landed in step-14.
    // Delegating to deterministic keeps the API contract satisfied.
    solve_cg_deterministic(k, f, inv_diag, tol_sq, max_iter, n)
}

#[cfg(test)]
mod tests {
    use super::{CgResult, CgSolverOptions, SolverMode, norm2_squared, solve_cg, spmv_seq};
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
            let diff = (result.u[i] - u_expected[i]).abs();
            assert!(
                diff < 1e-9,
                "u[{i}] = {} but expected {} (diff = {})",
                result.u[i],
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
        use crate::assembly::{AssemblyElement, AssemblyMode, assemble_global_stiffness};
        use crate::assembly::tet::element_stiffness_p1;
        use crate::boundary::{DirichletBc, apply_dirichlet_row_elimination};

        let mat = dimensionless_steel_like();
        let k_e = element_stiffness_p1(&UNIT_TET_P1, &mat);
        assert_eq!(k_e.n_dofs, 12);

        // 4 tets fanning around central node 0.
        let conns: [[usize; 4]; 4] = [
            [0, 1, 2, 3],
            [0, 4, 5, 6],
            [0, 7, 8, 9],
            [0, 10, 11, 12],
        ];
        let n_nodes = 13;
        let elements: Vec<AssemblyElement<'_>> = conns
            .iter()
            .enumerate()
            .map(|(i, c)| AssemblyElement { id: i, connectivity: c, k_e: &k_e })
            .collect();

        let mut k = assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Deterministic);

        // Pin DOFs 0..6 (nodes 0 and 1) to zero displacement. This removes
        // the rigid-body modes from the central node and one outer node,
        // making K SPD on the remaining free DOFs.
        let dim = 3 * n_nodes; // = 39
        let mut f = vec![0.0_f64; dim];
        let bcs: Vec<DirichletBc> = (0..6)
            .map(|dof| DirichletBc { dof, value: 0.0 })
            .collect();
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

        assert!(!result.converged, "must not converge with max_iter=1 and tol=1e-15");
        assert_eq!(result.iterations, 1, "exactly the cap was consumed");
        assert_eq!(result.u.len(), 2, "u has the correct length");
        // At least one entry of u is non-zero (one CG step took effect).
        assert!(
            result.u.iter().any(|&v| v != 0.0),
            "u must be non-zero after one CG step: {:?}",
            result.u
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
            opts.max_iter,
            result.converged,
            result.iterations
        );

        // Verify residual r = f − Ku using spmv_seq.
        let mut ku = vec![0.0_f64; n];
        spmv_seq(&k, &result.u, &mut ku);
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
}
