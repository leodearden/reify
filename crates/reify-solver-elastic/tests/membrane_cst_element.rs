//! Acceptance signals (PRD `docs/prds/v0_6/tensegrity-membrane.md` §5 + §8.2 G6,
//! task ζ) for the dedicated CST membrane element.
//!
//! Two integration goldens, mirroring `tests/bar_axial_deflection.rs`:
//!
//! 1. **In-plane multi-element patch test** — a CST exactly reproduces a
//!    constant-strain (linear) in-plane displacement field, so the interior-node
//!    displacement matches the exact field to machine precision (an EXACTNESS
//!    identity, asserted at 1e-9).
//! 2. **Pretensioned-membrane-under-pressure center deflection** (S11/S12) — a
//!    MESH-CONVERGENCE bound of the `N∇²w=−p` Fourier closed form, NOT an exact
//!    value (G6 honesty: a CST membrane under transverse pressure is an
//!    O(h²)-convergent approximation, not nodally exact).
//!
//! # In-plane patch test
//!
//! A flat unit-square patch `[0,1]²` triangulated into 4 CSTs fanning from a
//! non-centered interior vertex. The exact linear field `u_x=αx+βy`,
//! `u_y=γx+δy` is imposed on every boundary-node in-plane DOF; the out-of-plane
//! (z) DOF of every node is pinned (membrane `K_e` alone is transversely
//! singular). Assembling the membrane `K_e` and solving leaves the interior node
//! free in-plane; a CST reproduces constant strain exactly, so the solved
//! interior displacement equals the exact field to 1e-9.

use reify_solver_elastic::assembly::test_support::assert_close;
use reify_solver_elastic::constitutive::IsotropicElastic;
use reify_solver_elastic::{
    AssemblyElement, AssemblyMode, CgSolverOptions, DirichletBc, SolverMode,
    apply_dirichlet_row_elimination, assemble_global_stiffness, element_stiffness_membrane_cst,
    solve_cg,
};

/// (1) In-plane multi-element patch test: a CST exactly reproduces a constant-
/// strain linear field, so the interior node matches the exact field to 1e-9.
///
/// The patch fixture, the linear-field Dirichlet glue, and the
/// assemble→BC→solve→compare body are implemented in S10 via
/// `run_in_plane_patch_test`.
#[test]
fn in_plane_patch_reproduces_linear_field() {
    run_in_plane_patch_test();
}
