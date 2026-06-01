//! Self-stress & prestress-stability analysis kernel (Tensegrity T2).
//!
//! # PRD reference
//!
//! `docs/prds/v0_6/tensegrity-structures.md` §5 / Tier-2 leaf T2. This is the
//! layer-3 analysis kernel of the v0_6 tensegrity DAG: given a realised
//! geometry (`nodes`), a member topology (`members`), and per-member force
//! densities `q`, it reports the classical self-stress / mechanism / stability
//! verdict of the prestressed framework.
//!
//! # Method
//!
//! 1. **Equilibrium matrix** `A` (`d·N × m`, unit-direction convention
//!    `A·s = f` with `s` the member axial forces): column `i` for member
//!    `(j, k)` carries the unit direction `û = (x_k − x_j)/L` in node-`j`'s rows
//!    and `−û` in node-`k`'s rows, in node-major / axis-minor DOF order
//!    (`3a + α`) so `A`'s rows match `K_G = D ⊗ I₃` and the buckling kernel's
//!    `u[3·node + axis]` ordering.
//! 2. **Self-stress states** `s = nullity(A) = m − rank(A)` — a valid tensegrity
//!    needs `s ≥ 1` (PRD §5).
//! 3. **Infinitesimal mechanisms** `null(Aᵀ)` minus the rigid-body modes
//!    (3 translations + 3 infinitesimal rotations); the reported count is the
//!    rigid-excluded internal mechanism count.
//! 4. **Maxwell number** `m − d·N` (Calladine's identity, reported as the raw
//!    integer field).
//! 5. **Geometric/stress stiffness** `K_G = D ⊗ I₃` with `D = CᵀQC` reused
//!    verbatim from layer-2 ([`crate::form_find_free::assemble_force_density_matrix`]).
//!    No sign flip — `q` already encodes cable(+)/strut(−); this is the prestress
//!    energy Hessian (contrast the buckling kernel's `−K_g`).
//! 6. **Prestress stability**: reduced `K_G^red = Mᵀ K_G M` on the internal
//!    mechanism subspace `M`; prestress-stable iff `K_G^red ≻ 0`, tested by
//!    reusing the buckling dense eigensolver path
//!    ([`crate::eigensolve::solve_eigen_dense`]).
//! 7. **Super-stability** (Connelly): `D` PSD ∧ `rank(D) == N − d − 1`. The
//!    third condition (member directions not on a conic at infinity) is an
//!    intentionally-documented deferral.
//!
//! # Scope
//!
//! Kernel only: this module does not touch the `.ri` `constraint form.stable`
//! surface, the stdlib signature, or the reify-eval trampoline — exactly like
//! the T1a ([`crate::form_find`]) and T1b ([`crate::form_find_free`]) kernels
//! before it. See `plan.json` design_decisions for the scoping rationale.

#[cfg(test)]
mod tests {
    // Step-1 introduces `use super::*;` alongside the first RED unit test; an
    // empty glob import here would warn under the test cfg.
}
