//! Pin-jointed bar/cable geometric stiffness for `reify-solver-elastic`.
//!
//! Implements the geometric stiffness `K_g = (N/L)·(I − cc^T)` block kernel
//! for a 2-node, 6-DOF truss element, plus the per-member tangent stiffness
//! `bar_tangent_stiffness = K_e + K_g`.
//!
//! See PRD `docs/prds/v0_6/tensegrity-structures.md` §6 task T3a.
