//! P2 (quadratic, 10-node) tetrahedron consistent mass-matrix kernel.
//!
//! Task 4066 — "P2-tet modal frequencies": the P1 constant-strain tet
//! (`crate::mass_matrix::consistent_element_mass_tet_p1`) locks in bending,
//! flooring slender-beam modal-frequency error several percent above the 2%
//! aspirational target. The fix mirrors the P2 buckling path (task 4052,
//! `solve_buckling_kernel_p2`): quadratic shape functions resolve bending
//! curvature, so this module supplies the one missing primitive — the P2
//! **consistent mass** `M_e` — to pair with the existing P2 stiffness
//! (`crate::element_stiffness` at `ElementOrder::P2`) in the modal eigenproblem
//! `K φ = λ M φ`.
//!
//! The element matrix shares the row-major `(3·node + axis)` DOF layout of
//! [`crate::assembly::ElementStiffness`] (here 30 DOFs = 10 nodes × 3 axes), so
//! the global mass matrix `M` is assembled by handing each element `M_e` to the
//! existing [`crate::assemble_global_stiffness`] scatter primitive — the
//! assembler is agnostic to `K` vs `K_g` vs `M`.
//!
//! # Why exact degree-4 integration (the central technical point)
//!
//! The mass integrand is `N_a · N_b`. P2 shape functions are quadratic, so the
//! product is a **degree-4** polynomial — unlike the P2 *stiffness* integrand
//! `∇N · ∇N` (degree-2), for which the 4-point Stroud rule on
//! [`crate::elements::tet_p2::TetP2`] is exact. Re-using that degree-2 rule for
//! the mass would make the 10×10 reference Gram matrix rank ≤ 4, hence the
//! 30×30 `M_e` rank ≤ 12 < 30 — singular and **not** positive-definite. The
//! generalized modal eigensolve (`crate::solve_eigen_dense` /
//! `solve_eigen_shift_invert`) factors `M` via Cholesky and therefore requires
//! `M` SPD. So this kernel integrates `N_a · N_b` **exactly** to degree 4 via
//! closed-form barycentric monomial integration
//! `∫_T λ0^i λ1^j λ2^k λ3^l dV = V · (i! j! k! l! · 3!) / ((i+j+k+l+3)!)`,
//! which is exact-by-construction for an affine (straight-edge) tet and mirrors
//! the P1 closed-form precedent. The linear-velocity kinetic-energy unit test
//! (`vᵀ M v = ρ ∫ v² dV`) is the gate that fails on any under-degree rule.
