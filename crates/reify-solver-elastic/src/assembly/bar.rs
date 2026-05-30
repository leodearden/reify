//! Pin-jointed bar/cable element stiffness for `reify-solver-elastic`.
//!
//! Implements the elastic stiffness `K_e = (EA/L)·cc^T` block kernel for a
//! 2-node, 6-DOF truss element, plus the `BarSection` input struct.
//!
//! See PRD `docs/prds/v0_6/tensegrity-structures.md` §6 task T3a.
