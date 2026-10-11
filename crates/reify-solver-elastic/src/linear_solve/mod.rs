//! The tiered SPD linear-solve seam: one entry point that solves `K·u = f`
//! for a post-boundary-condition stiffness `K`, by a direct sparse Cholesky
//! or by preconditioned CG. Contract: PRD
//! `docs/prds/v0_6/elastic-static-solver-performance.md` §5 C1–C4.

mod system;

pub use system::{SpdSystem, SystemInvalid};
