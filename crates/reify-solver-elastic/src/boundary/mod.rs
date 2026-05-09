//! Boundary condition application for the linear-elastostatic FEA solver.
//!
//! See PRD `docs/prds/v0_3/structural-analysis-fea.md` tasks #10 (Dirichlet)
//! and #11 (Neumann). Each BC kind lives in its own submodule so the two
//! surfaces stay narrow and independent.
//!
//! # Additive accumulation
//!
//! The three Neumann `apply_*` primitives (`apply_point_load`,
//! `apply_body_force`, `apply_traction_load`) are designed to compose
//! additively into a shared `&mut [f64]` global load vector. Callers should:
//!
//! ```ignore
//! let mut f = vec![0.0; 3 * n_nodes];
//! for load in &surface_tractions { apply_traction_load(&mut f, ...) }
//! for load in &body_forces       { apply_body_force(&mut f, ...)    }
//! for load in &point_loads       { apply_point_load(&mut f, ...)    }
//! ```
//!
//! Never pass a pre-populated `f` expecting the function to overwrite — each
//! function uses `+=`.

pub mod dirichlet;
pub mod neumann;

pub use dirichlet::{DirichletBc, apply_dirichlet_row_elimination};
pub use neumann::{FaceOrder, apply_body_force, apply_point_load, apply_traction_load};
