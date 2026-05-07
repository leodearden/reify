//! Element-level stiffness assembly for the linear-elastostatic FEA solver.
//!
//! See PRD `docs/prds/v0_3/structural-analysis-fea.md` task #8. This module
//! ships the per-element stiffness assembly machinery — the dense
//! `K_e = ∫_Ω_e BᵀDB dV` integrand — for both P1 and P2 tetrahedra. Global
//! sparse-matrix assembly via faer-rs is PRD task #9's job and consumes
//! [`ElementStiffness`] row-major.

pub mod tet;
