//! Boundary condition application for the linear-elastostatic FEA solver.
//!
//! See PRD `docs/prds/v0_3/structural-analysis-fea.md` tasks #10 (Dirichlet)
//! and #11 (Neumann, future). Each BC kind lives in its own submodule so the
//! two surfaces stay narrow and independent.

pub mod dirichlet;

pub use dirichlet::{DirichletBc, apply_dirichlet_row_elimination};
