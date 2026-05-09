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
}
