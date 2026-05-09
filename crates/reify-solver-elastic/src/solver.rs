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
